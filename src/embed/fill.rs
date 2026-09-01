//! The deliberate fill of the embedding layer (DEC-034).
//!
//! Every query embeds what it needs on the spot, which is right for a scope
//! somebody is about to read and wrong for a corpus: at the measured ~295
//! units/second a monorepo is about two hours, and `search` refuses that rather
//! than running it inside a tool call (DEC-032). This is where a caller says
//! yes to it.
//!
//! Three properties, and they are the whole design:
//!
//! - **It only ever embeds what has nothing.** The work is keyed by text, so a
//!   second run over a warm scope is one query and a report.
//! - **It commits as it goes.** A Ctrl-C, a budget stop or a machine going to
//!   sleep keeps every batch that finished; the next run continues from there.
//! - **It measures its own rate.** The estimate before the run is the projection
//!   DEC-032 refuses on; every line after it is this machine, this corpus, now.

use super::{Embedder, Prefer, Text, config_key, text_of};
use crate::store::Store;
use anyhow::Result;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::time::Instant;

/// Texts per commit.
///
/// At the measured ONNX rate this is under two seconds of work, which is what
/// an interrupted run stands to lose, and it is large enough that the pool is
/// saturated and the commit between batches is noise. A caller feels it as how
/// often the progress line moves.
const BATCH: usize = 500;

/// How often the progress line may repeat, in seconds.
///
/// Two numbers because there are two readers. At a terminal the line is
/// rewritten in place, so it can move often enough to prove the run is alive;
/// redirected it is one line per report, and a two-hour fill reporting every
/// two seconds would be three thousand of them in somebody's log.
const REPORT_EVERY_TTY: f64 = 2.0;
const REPORT_EVERY_PIPED: f64 = 30.0;

/// What one fill did. Every number is work, not contents.
#[derive(Debug, Default, serde::Serialize)]
pub struct Filled {
    /// Units in scope.
    pub units: usize,
    /// Distinct texts those units embed as. Lower than `units` wherever two
    /// units share a summary or a name — one vector serves both.
    pub texts: usize,
    /// Texts that already had a vector when this run started.
    pub warm: usize,
    /// Texts embedded by this run.
    pub embedded: usize,
    /// Texts still without a vector. Zero means every query over this scope
    /// now answers warm.
    pub remaining: usize,
    pub seconds: f64,
    /// Texts per second, measured by this run rather than projected. Null
    /// where the run embedded nothing, because a rate over no work is not a
    /// measurement.
    pub rate: Option<f64>,
    /// `budget` or `cancelled` where the run stopped early, null where it
    /// finished the scope.
    pub stopped: Option<&'static str>,
    /// Which embedder filled it — vectors from two are separate indexes
    /// (DEC-005), so a fill is only warm for the embedder that ran it.
    pub embedder: &'static str,
}

/// Embed every text in `scope` that has no vector yet.
///
/// `budget` is a wall-clock ceiling in seconds, checked between batches; `None`
/// runs to the end of the scope. Unlike a query, this never refuses — being
/// asked for is what makes the bill consented to.
pub fn fill(
    store: &mut Store,
    root: &str,
    scope: Option<&str>,
    embedder: &dyn Embedder,
    budget: Option<f64>,
) -> Result<Filled> {
    let config = config_key(embedder.kind(), embedder.model());
    let stored = store.all_summaries()?;

    let mut counts = Filled {
        embedder: embedder.kind(),
        ..Filled::default()
    };
    // One entry per distinct text, in the order the units appear, so two runs
    // of the same `--budget` work through the scope rather than re-rolling the
    // same dice. `text_of` is the same rule `search` embeds by, which is what
    // makes what this buys the thing that gets looked up.
    let mut keys: HashSet<u64> = HashSet::new();
    let mut texts: Vec<(u64, String)> = Vec::new();
    for located in store.units(root)? {
        if !scope.is_none_or(|s| crate::paths::under(&located.path, s)) {
            continue;
        }
        counts.units += 1;
        let summary = located.unit.norm_hash.and_then(|norm_hash| {
            let ctx = crate::summary::Context::of(&located.unit).hash();
            stored.get(&(norm_hash, ctx))
        });
        let Some(Text { key, text, .. }) = text_of(&located.unit, summary, Prefer::Best) else {
            continue;
        };
        if keys.insert(key) {
            texts.push((key, text));
        }
    }
    counts.texts = texts.len();

    let have = store.vectors(config, &keys)?;
    let todo: Vec<(u64, String)> = texts
        .into_iter()
        .filter(|(key, _)| !have.contains_key(key))
        .collect();
    counts.warm = counts.texts - todo.len();
    counts.remaining = todo.len();
    if todo.is_empty() {
        return Ok(counts);
    }

    // The same projection a query would have refused on (DEC-032), stated
    // rather than enforced: this command *is* the consent, so the only thing
    // left to do with the number is show it.
    eprintln!(
        "contour: {} unit(s) in scope, {} text(s) to embed — about {} with the {} embedder \
         on {} thread(s). Everything embedded is kept; interrupt and re-run to continue.",
        counts.units,
        todo.len(),
        super::about(super::projected_seconds(embedder.kind(), todo.len())),
        embedder.kind(),
        rayon::current_num_threads().max(1),
    );

    let started = Instant::now();
    let mut reported = started;
    let mut widest = 0usize;
    let tty = std::io::stderr().is_terminal();
    let (every, end) = match tty {
        true => (REPORT_EVERY_TTY, "\r"),
        false => (REPORT_EVERY_PIPED, "\n"),
    };
    for batch in todo.chunks(BATCH) {
        let batch_texts: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();
        let vectors = super::embed_all(None, &batch_texts);
        // A cancelled run comes back holding empty vectors for what it did not
        // reach, and storing those would poison the cache with answers no
        // embedder produced — the same guard the query path keeps.
        if crate::cancel::current().cancelled() {
            counts.stopped = Some("cancelled");
            break;
        }
        let fresh: Vec<(u64, Vec<f32>)> = batch
            .iter()
            .map(|(key, _)| *key)
            .zip(vectors)
            .filter(|(_, vec)| !vec.is_empty())
            .collect();
        // Committed per batch, not at the end: what is bought is kept.
        store.put_vectors(config, &fresh)?;
        counts.embedded += fresh.len();
        counts.remaining -= fresh.len();

        let elapsed = started.elapsed().as_secs_f64();
        // Checked after a batch rather than before one, so any budget at all
        // buys some work: a run that stops having embedded nothing has not
        // honoured a budget, it has ignored the request. The overshoot is one
        // batch, which is what `BATCH` is sized against.
        if counts.remaining > 0 && budget.is_some_and(|budget| elapsed >= budget) {
            counts.stopped = Some("budget");
            break;
        }
        if reported.elapsed().as_secs_f64() >= every {
            reported = Instant::now();
            // The rate is this run's own, so the estimate improves as it goes
            // and reflects whatever else the machine is doing.
            let rate = counts.embedded as f64 / elapsed.max(f64::MIN_POSITIVE);
            let line = format!(
                "contour: {}/{} embedded — {} text(s)/s, about {} left",
                counts.embedded,
                todo.len(),
                rate.round() as u64,
                super::about(counts.remaining as f64 / rate.max(f64::MIN_POSITIVE)),
            );
            // Padded to the longest line so far, because "about 9 second(s)"
            // is shorter than "about 10 minute(s)" and a carriage return
            // leaves whatever it does not overwrite.
            widest = widest.max(line.chars().count());
            eprint!("{line:<widest$}{end}");
            let _ = std::io::stderr().flush();
        }
    }

    // The in-place line is left where it is otherwise, with the summary
    // written over its tail.
    if tty {
        eprintln!();
    }
    counts.seconds = round1(started.elapsed().as_secs_f64());
    counts.rate = (counts.embedded > 0 && counts.seconds > 0.0)
        .then(|| round1(counts.embedded as f64 / counts.seconds));
    Ok(counts)
}

/// A rate and an elapsed time built from a clock and a count have about three
/// significant figures; printing seventeen claims evidence neither has.
fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store holding one file's units, which is everything `fill` reads.
    fn indexed(source: &str) -> (Store, String) {
        let mut store = Store::open_in_memory().unwrap();
        let oid = crate::scan::hash_blob(source.as_bytes());
        let files: crate::scan::Files = [("app.rb".to_string(), oid.clone())].into_iter().collect();
        let blob = crate::ruby::units(source.as_bytes());
        store.write("/r", &files, vec![(oid, blob)]).unwrap();
        (store, "/r".to_string())
    }

    /// The property the whole command rests on: a second run buys nothing.
    /// Without it, "run it once and the corpus is warm forever" is a claim
    /// nothing enforces — and the way to break it is to embed a text under a
    /// key nobody looks up, which this would catch as a fill that never ends.
    #[test]
    fn a_second_fill_over_the_same_scope_is_a_no_op() {
        let (mut store, root) =
            indexed("class Widget\n  def save(record)\n    record.persist\n  end\nend\n");
        let embedder = super::super::HashEmbedder;

        let first = fill(&mut store, &root, None, &embedder, None).unwrap();
        assert!(first.embedded > 0, "a cold scope has something to embed");
        assert_eq!(first.remaining, 0);

        let second = fill(&mut store, &root, None, &embedder, None).unwrap();
        assert_eq!(second.embedded, 0, "it bought what was already bought");
        assert_eq!(second.warm, first.texts);
        assert_eq!(second.remaining, 0);
    }

    /// A budget stop keeps what it bought. The per-batch commit is what makes
    /// that true; a fill that wrote once at the end would lose the whole run.
    ///
    /// The budget is smaller than any batch can take, so the run stops at the
    /// first boundary — which is the granularity the promise is made at.
    #[test]
    fn a_budget_that_stops_the_run_keeps_every_batch_it_finished() {
        let mut source = String::from("class Widget\n");
        for i in 0..(BATCH * 2) {
            source.push_str(&format!(
                "  def handler_{i}(record)\n    record.at({i})\n  end\n"
            ));
        }
        source.push_str("end\n");
        let (mut store, root) = indexed(&source);
        let embedder = super::super::HashEmbedder;

        let stopped = fill(&mut store, &root, None, &embedder, Some(1e-9)).unwrap();
        assert_eq!(stopped.stopped, Some("budget"));
        assert_eq!(stopped.embedded, BATCH, "one batch, committed");
        assert!(stopped.remaining > 0);

        let rest = fill(&mut store, &root, None, &embedder, None).unwrap();
        assert_eq!(rest.warm, BATCH, "the stopped run's batch survived it");
        assert_eq!(rest.remaining, 0);
    }
}
