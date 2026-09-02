//! VENDORED from rq `src/profile.rs`, MIT, same author. Trimmed: rq's
//! "slowest units of work" list is dropped, because contour has no
//! per-input pathology to locate yet — its phases are corpus-sized reads,
//! not one bad file.
//!
//! Where the wall clock went, for `--profile`.
//!
//! A field report on a monorepo had to attach an OS stack sampler to a
//! release binary to learn that a third of a scoped query was blocked in
//! page reads. This is that answer from the inside: a flat list of named
//! phases, a total, and counts saying *how much work there was* — which a
//! duration alone cannot distinguish from work that was merely slow.
//!
//! **Phases do not nest.** Each one accumulates the wall time of one segment
//! of a mostly sequential pipeline, so the phases plus the unaccounted
//! remainder are the total. A span inside another span would double-count,
//! and the report would stop adding up.
//!
//! **A phase is a category of work, not an occurrence.** Two spans with one
//! name sum into one row carrying `xN`, because the question this answers is
//! "how much of the run was index I/O", and a query that reads the unit table
//! twice should say so on one line rather than hide it across two.
//!
//! Off by default and free when off: a span reads no clock, takes no lock and
//! allocates nothing unless profiling is on, so the cost on the query path is
//! a relaxed atomic load per phase.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static PHASES: Mutex<Vec<Phase>> = Mutex::new(Vec::new());
static COUNTERS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

/// One measured phase, summed over every span that carried its name.
pub(crate) struct Phase {
    pub name: &'static str,
    pub elapsed: Duration,
    /// How many spans carried this name.
    pub times: u32,
    /// What the phase did — rows read, texts embedded, units scored. The first
    /// occurrence's, because a repeated phase is repeated work of one kind.
    pub note: Option<String>,
}

/// Enable profiling from the `--profile` flag; `CONTOUR_PROFILE` in the
/// environment also enables it, so a binary invoked by something else — a
/// script, a Makefile, an editor — can be measured without editing the command
/// line that runs it.
pub fn enable_from(flag: bool) {
    let on = flag || std::env::var_os("CONTOUR_PROFILE").is_some();
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Start timing a phase. The returned span records it when dropped; with
/// profiling off it is inert.
pub(crate) fn span(name: &'static str) -> Span {
    Span {
        name,
        start: enabled().then(Instant::now),
        note: None,
    }
}

/// Add `n` to a named counter. Additive: repeated calls with one name sum.
pub(crate) fn count(name: &'static str, n: u64) {
    if !enabled() {
        return;
    }
    if let Ok(mut counters) = COUNTERS.lock() {
        match counters.iter_mut().find(|(k, _)| *k == name) {
            Some((_, v)) => *v += n,
            None => counters.push((name, n)),
        }
    }
}

pub(crate) struct Span {
    name: &'static str,
    start: Option<Instant>,
    note: Option<String>,
}

impl Span {
    /// Attach detail to this phase. The closure runs only when profiling is
    /// on, so formatting a count is never paid for in a normal run.
    pub(crate) fn note(&mut self, f: impl FnOnce() -> String) {
        if self.start.is_some() {
            self.note = Some(f());
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        let elapsed = start.elapsed();
        if let Ok(mut phases) = PHASES.lock() {
            match phases.iter_mut().find(|p| p.name == self.name) {
                Some(phase) => {
                    phase.elapsed += elapsed;
                    phase.times += 1;
                }
                None => phases.push(Phase {
                    name: self.name,
                    elapsed,
                    times: 1,
                    note: self.note.take(),
                }),
            }
        }
    }
}

/// Every phase recorded so far, in the order each first began. Drains.
fn phases() -> Vec<Phase> {
    PHASES
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

/// Counters recorded so far. Drains.
fn counters() -> Vec<(&'static str, u64)> {
    COUNTERS
        .lock()
        .map(|mut c| std::mem::take(&mut *c))
        .unwrap_or_default()
}

/// The report as stderr-ready lines. Empty when nothing was measured.
///
/// The `unaccounted` row is the point of the thing: without it a reader cannot
/// tell a phase list that explains the run from one that explains a third of
/// it, and the field report this exists for was about the part nobody had
/// named.
pub fn report(total: Duration) -> Vec<String> {
    let phases = phases();
    let counters = counters();
    if phases.is_empty() && counters.is_empty() {
        return Vec::new();
    }
    let w = phases
        .iter()
        .map(|p| p.name.len())
        .chain(std::iter::once("unaccounted".len()))
        .max()
        .unwrap_or(11);
    let measured: Duration = phases.iter().map(|p| p.elapsed).sum();
    let mut out: Vec<String> = phases
        .iter()
        .map(|p| {
            format!(
                "  {:<w$}  {:>8}  {:>5}  {}",
                p.name,
                ms(p.elapsed),
                share(p.elapsed, total),
                detail(p),
                w = w
            )
            .trim_end()
            .to_string()
        })
        .collect();
    // Phases that add to more than the run are phases that nested, which is
    // the one way this table can lie. Say so rather than clamping the
    // remainder to zero and looking complete — that is how it hid the first
    // time, when a per-worker embedder load was timed inside `embed`.
    match measured > total {
        true => out.push(format!(
            "  {:<w$}  {:>8}         phases overlap; some span nests",
            "OVERLAP",
            ms(measured - total),
            w = w
        )),
        false => {
            let rest = total - measured;
            out.push(format!(
                "  {:<w$}  {:>8}  {:>5}",
                "unaccounted",
                ms(rest),
                share(rest, total),
                w = w
            ));
        }
    }
    out.push(format!("  {:<w$}  {:>8}", "total", ms(total), w = w));
    for (name, v) in &counters {
        out.push(format!("  {name:<w$}  {v:>8}", w = w));
    }
    out
}

/// The same report as one compact JSON object, for storing a baseline and
/// diffing two runs.
pub fn json(total: Duration) -> String {
    let phases = phases();
    let counters = counters();
    let measured: Duration = phases.iter().map(|p| p.elapsed).sum();
    let body: Vec<String> = phases
        .iter()
        .map(|p| {
            let note = match &p.note {
                Some(n) => quote(n),
                None => "null".to_string(),
            };
            format!(
                "{{\"name\":{},\"ms\":{:.3},\"times\":{},\"note\":{note}}}",
                quote(p.name),
                millis(p.elapsed),
                p.times
            )
        })
        .collect();
    // `counters` is always present, empty when a run recorded none, so a
    // consumer reads it without probing for the key.
    let counts: Vec<String> = counters
        .iter()
        .map(|(k, v)| format!("{}:{v}", quote(k)))
        .collect();
    format!(
        "{{\"total_ms\":{:.3},\"unaccounted_ms\":{:.3},\"phases\":[{}],\"counters\":{{{}}}}}",
        millis(total),
        millis(total.saturating_sub(measured)),
        body.join(","),
        counts.join(",")
    )
}

/// A JSON string literal. Notes are arbitrary text, so they go through serde
/// rather than hand-rolled quoting.
/// A phase's note, prefixed with how many times it fired when that is more
/// than once — the fact a reader needs to see a doubled read as doubled.
fn detail(phase: &Phase) -> String {
    let note = phase.note.as_deref().unwrap_or_default();
    match phase.times {
        0 | 1 => note.to_string(),
        n => format!("x{n}  {note}").trim_end().to_string(),
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn ms(d: Duration) -> String {
    format!("{:.1}ms", millis(d))
}

/// A share of the total, to the precision a single timed run has: whole
/// percent. More digits would dress up run-to-run noise as measurement.
fn share(part: Duration, total: Duration) -> String {
    if total.is_zero() {
        return "-".to_string();
    }
    format!("{:.0}%", 100.0 * part.as_secs_f64() / total.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ENABLED` and `PHASES` are process-global and cargo runs tests as
    /// threads in one process, so these would otherwise interleave — the "off"
    /// test seeing the "on" test's flag and running a closure that panics on
    /// purpose.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Poison-tolerant: one failing test shouldn't cascade into the other.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_span_is_inert_when_profiling_is_off() {
        let _guard = serial();
        ENABLED.store(false, Ordering::Relaxed);
        let _ = (phases(), counters());
        let mut s = span("off");
        s.note(|| panic!("the note closure must not run when disabled"));
        drop(s);
        count("off", 1);
        assert!(phases().is_empty());
        assert!(counters().is_empty());
    }

    #[test]
    fn an_enabled_span_records_its_name_and_note_and_counters_sum() {
        let _guard = serial();
        enable_from(true);
        let _ = (phases(), counters());
        {
            let mut s = span("on");
            s.note(|| "9 rows".to_string());
        }
        count("rows read", 40);
        count("rows read", 2);
        let recorded = phases();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].name, "on");
        assert_eq!(recorded[0].note.as_deref(), Some("9 rows"));
        assert!(phases().is_empty(), "phases() drains");
        assert_eq!(counters(), vec![("rows read", 42)]);
        ENABLED.store(false, Ordering::Relaxed);
    }

    /// The whole point of the report: a phase list that explains a third of
    /// the run must not read like one that explains all of it.
    #[test]
    fn the_report_names_what_it_could_not_account_for() {
        let _guard = serial();
        enable_from(true);
        let _ = (phases(), counters());
        PHASES.lock().unwrap().push(Phase {
            name: "read",
            elapsed: Duration::from_millis(25),
            times: 1,
            note: None,
        });
        let lines = report(Duration::from_millis(100));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("read") && l.contains("25%"))
        );
        let rest = lines.iter().find(|l| l.contains("unaccounted")).unwrap();
        assert!(rest.contains("75%"), "{rest}");
        ENABLED.store(false, Ordering::Relaxed);
    }

    /// Only the embedder a *caller* builds is a phase. `embed::embed_all`
    /// builds one per rayon worker, inside the `embed` phase and on every core
    /// at once, and timing those nested a phase inside a phase — the first
    /// real profile of a search reported shares adding to 142% of the run.
    #[test]
    fn only_the_query_embedder_is_a_phase() {
        let _guard = serial();
        enable_from(true);
        let _ = (phases(), counters());

        drop(crate::embed::default_embedder(
            None,
            crate::embed::Workload::Bulk,
        ));
        assert!(phases().is_empty(), "a worker's embedder is not a phase");

        drop(crate::embed::default_embedder(
            None,
            crate::embed::Workload::Query,
        ));
        let recorded = phases();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].name, "embedder load");
        ENABLED.store(false, Ordering::Relaxed);
    }

    /// The one way this table can lie: phases that nested, so they add to more
    /// than the run and the remainder clamps to a reassuring zero.
    #[test]
    fn phases_adding_to_more_than_the_run_say_so() {
        let _guard = serial();
        enable_from(true);
        let _ = (phases(), counters());
        PHASES.lock().unwrap().push(Phase {
            name: "nested",
            elapsed: Duration::from_millis(150),
            times: 2,
            note: None,
        });
        let lines = report(Duration::from_millis(100));
        assert!(lines.iter().any(|l| l.contains("OVERLAP")), "{lines:?}");
        assert!(
            !lines.iter().any(|l| l.contains("unaccounted")),
            "{lines:?}"
        );
        ENABLED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn the_json_report_carries_the_same_two_numbers() {
        let _guard = serial();
        enable_from(true);
        let _ = (phases(), counters());
        PHASES.lock().unwrap().push(Phase {
            name: "read",
            elapsed: Duration::from_millis(25),
            times: 1,
            note: Some("3 rows".to_string()),
        });
        count("rows", 3);
        let out = json(Duration::from_millis(100));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["total_ms"], 100.0);
        assert_eq!(parsed["unaccounted_ms"], 75.0);
        assert_eq!(parsed["phases"][0]["name"], "read");
        assert_eq!(parsed["phases"][0]["note"], "3 rows");
        assert_eq!(parsed["counters"]["rows"], 3);
        ENABLED.store(false, Ordering::Relaxed);
    }
}
