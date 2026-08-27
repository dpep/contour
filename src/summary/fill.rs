//! Budgeted, on-demand summarization (DEC-009).
//!
//! Three things this loop is careful about, all of them because the work costs
//! money rather than milliseconds:
//!
//! - **Buy each answer once.** Units are grouped by their full cache key
//!   before anything is sent, so a clone in identical context is one call.
//! - **Never lose what was bought.** Each answer is written as it arrives, not
//!   batched to the end, so an interrupt or a rate limit keeps everything paid
//!   for so far.
//! - **Never buy a wrong answer.** A stored summary is keyed by the body it
//!   describes, so summarizing the wrong lines would poison that key
//!   permanently. See `slice`.

use super::{Context, Request, Summarizer, Usage};
use crate::store::{Store, SummaryKey};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// What one fill did. Every number is work, not contents.
#[derive(Debug, Default, serde::Serialize)]
pub struct Filled {
    /// Answers bought in this run.
    pub summarized: usize,
    /// Units covered without a call, because a clone in identical context was
    /// summarized in this run or an earlier one.
    pub shared: usize,
    pub failed: usize,
    /// Distinct answers still unbought in this scope. Zero means the scope is
    /// complete; anything else is what a bigger `--budget` would work through.
    pub remaining: usize,
    pub usage: Usage,
}

/// How much of a scope has been summarized.
#[derive(Debug, Default, serde::Serialize)]
pub struct Coverage {
    /// Units with a body to summarize. Macro-generated units have none, so
    /// they are not counted as missing — a denominator that includes work that
    /// can never be done never reaches 100%.
    pub summarizable: usize,
    pub summarized: usize,
}

impl Coverage {
    /// `complete` / `warming` / `none`, travelling with every answer per
    /// DEC-009. The fraction travels with it, because the label alone cannot
    /// tell a reader whether "warming" means 2% or 98%.
    pub fn state(&self) -> &'static str {
        match (self.summarized, self.summarizable) {
            (0, _) => "none",
            (done, total) if done >= total => "complete",
            _ => "warming",
        }
    }
}

/// Summarize up to `budget` distinct answers in `scope`.
pub fn fill(
    store: &mut Store,
    root: &Path,
    scope: Option<&str>,
    summarizer: &dyn Summarizer,
    budget: usize,
) -> Result<Filled> {
    let prompt = super::PROMPT_VERSION;
    let model = summarizer.model().to_string();
    let root_str = root.to_string_lossy().into_owned();

    // One entry per distinct answer, with the units it would serve. Grouping
    // before spending is the dedup DEC-003 promises: an exact clone in
    // identical context is one call, however many places it appears.
    let mut wanted: HashMap<(u64, u64), Vec<Located>> = HashMap::new();
    for located in store.units(&root_str)? {
        let Some(norm_hash) = located.unit.norm_hash else {
            continue;
        };
        if !scope.is_none_or(|s| crate::paths::under(&located.path, s)) {
            continue;
        }
        let context = Context::of(&located.unit);
        wanted
            .entry((norm_hash, context.hash()))
            .or_default()
            .push(Located {
                path: located.path,
                line: located.unit.line,
                end_line: located.unit.end_line,
                context,
            });
    }

    let have: HashSet<(u64, u64)> = store.have_summaries(prompt, &model, crate::store::VIA_API)?;
    let mut todo: Vec<((u64, u64), Vec<Located>)> = wanted
        .into_iter()
        .filter(|(key, _)| !have.contains(key))
        .collect();
    // Deterministic order, so two runs of `--budget 100` work through the
    // corpus rather than re-rolling the same dice.
    todo.sort_by(|a, b| (&a.1[0].path, a.1[0].line, a.0).cmp(&(&b.1[0].path, b.1[0].line, b.0)));

    let mut counts = Filled {
        remaining: todo.len(),
        ..Filled::default()
    };
    for ((norm_hash, ctx_hash), units) in todo.into_iter().take(budget) {
        let here = &units[0];
        let source = match slice(root, &here.path, here.line, here.end_line, norm_hash) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("contour: skipped {}:{} — {err}", here.path, here.line);
                counts.failed += 1;
                continue;
            }
        };
        let request = Request {
            source,
            context: here.context.clone(),
        };
        match summarizer.summarize(&request) {
            Ok((summary, usage)) => {
                // Written now, not at the end: an interrupt after this point
                // must not throw away an answer that has already been paid for.
                let key = SummaryKey {
                    norm_hash,
                    ctx_hash,
                    prompt,
                    model: &model,
                    via: crate::store::VIA_API,
                };
                // A refused answer is one unit's failure, not the run's. The
                // store gates what it accepts, and a batch that has already
                // spent money on fifty good answers must not throw them away
                // over the fifty-first.
                match store.put_summary(&key, &summary) {
                    Ok(()) => {
                        counts.summarized += 1;
                        counts.shared += units.len() - 1;
                        counts.remaining -= 1;
                        counts.usage += usage;
                    }
                    Err(err) => {
                        eprintln!("contour: {}:{} refused — {err:#}", here.path, here.line);
                        counts.failed += 1;
                        // The call still happened, so the cost still happened.
                        counts.usage += usage;
                    }
                }
            }
            Err(err) => {
                eprintln!("contour: {}:{} — {err:#}", here.path, here.line);
                counts.failed += 1;
            }
        }
    }
    Ok(counts)
}

/// One unit still needing a summary, with everything a summarizer needs.
#[derive(Debug, serde::Serialize)]
pub struct Pending {
    pub id: String,
    pub path: String,
    pub line: u32,
    pub end_line: u32,
    /// The context block the prompt renders, so a contributed summary is
    /// written against the same information the API path would have seen.
    pub context: String,
    pub source: String,
}

/// Units in scope that no summary covers yet, with their source.
///
/// What a session needs to do an explicit fill. Deduplicated by cache key, so
/// a clone at ten call sites is offered once — a session should no more pay
/// for it twice than contour should.
pub fn pending(
    store: &Store,
    root: &Path,
    scope: Option<&str>,
    model: &str,
    limit: usize,
) -> Result<Vec<Pending>> {
    let root_str = root.to_string_lossy().into_owned();
    let have_api = store.have_summaries(super::PROMPT_VERSION, model, crate::store::VIA_API)?;
    let have_mcp = store.have_summaries(
        super::contributed::CONTRIBUTED_PROMPT_VERSION,
        model,
        crate::store::VIA_MCP,
    )?;

    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut out = Vec::new();
    for located in store.units(&root_str)? {
        let Some(norm_hash) = located.unit.norm_hash else {
            continue;
        };
        if !scope.is_none_or(|s| crate::paths::under(&located.path, s)) {
            continue;
        }
        let context = Context::of(&located.unit);
        let key = (norm_hash, context.hash());
        if have_api.contains(&key) || have_mcp.contains(&key) || !seen.insert(key) {
            continue;
        }
        // A body that no longer matches the index is skipped rather than
        // offered: a session summarizing it would produce something the store
        // would then refuse, which wastes the session's tokens.
        let Ok(source) = slice(
            root,
            &located.path,
            located.unit.line,
            located.unit.end_line,
            norm_hash,
        ) else {
            continue;
        };
        out.push(Pending {
            id: located.unit.id(),
            path: located.path,
            line: located.unit.line,
            end_line: located.unit.end_line,
            context: context.render(),
            source,
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

/// How much of a scope one `(model, via)` has bought.
///
/// The calibration question (DEC-018) and the one a bulk fill works against —
/// *not* the same question as [`answerable`], which is what a query can
/// actually use. `--status` reports both, because reporting one of them under
/// the other's name is how it came to say `none` about a corpus `search` was
/// answering from.
pub fn coverage(store: &Store, root: &str, model: &str, via: &str) -> Result<Coverage> {
    let prompt = match via {
        crate::store::VIA_MCP => super::contributed::CONTRIBUTED_PROMPT_VERSION,
        _ => super::PROMPT_VERSION,
    };
    let have = store.have_summaries(prompt, model, via)?;
    let mut counts = Coverage::default();
    for located in store.units(root)? {
        let Some(norm_hash) = located.unit.norm_hash else {
            continue;
        };
        counts.summarizable += 1;
        if have.contains(&(norm_hash, Context::of(&located.unit).hash())) {
            counts.summarized += 1;
        }
    }
    Ok(counts)
}

/// How much of a scope a *query* can answer from meaning.
///
/// Any model, any provenance — which is exactly what `search` reads, because
/// refusing to answer from a summary somebody already paid for would hide
/// work that exists. The two must agree, and a test asserts they do.
pub fn answerable(store: &Store, root: &str) -> Result<Coverage> {
    let stored = store.all_summaries()?;
    let mut counts = Coverage::default();
    for located in store.units(root)? {
        let Some(norm_hash) = located.unit.norm_hash else {
            continue;
        };
        counts.summarizable += 1;
        if stored.contains_key(&(norm_hash, Context::of(&located.unit).hash())) {
            counts.summarized += 1;
        }
    }
    Ok(counts)
}

/// One unit's location, carried alongside the context so the loop does not
/// rebuild it.
struct Located {
    path: String,
    line: u32,
    end_line: u32,
    context: Context,
}

/// The method's source, verified to be the method.
///
/// The index records line numbers against the blob it read; the working tree
/// may have moved since. Slicing blindly would send the wrong lines and store
/// the answer under the *right* body's key — a wrong summary that is paid for,
/// permanently cached, and indistinguishable from a correct one.
///
/// So the slice is re-parsed and its hash compared. A `norm_hash` covers only
/// the def's own parameters and body, so a method parsed alone hashes
/// identically to the same method parsed in its file — which is what makes
/// this check exact rather than approximate.
pub(crate) fn slice(
    root: &Path,
    path: &str,
    line: u32,
    end_line: u32,
    norm_hash: u64,
) -> Result<String> {
    let text = std::fs::read_to_string(root.join(path))?;
    let lines: Vec<&str> = text.lines().collect();
    let (from, to) = (line as usize - 1, end_line as usize);
    anyhow::ensure!(
        from < lines.len() && to <= lines.len(),
        "the file is shorter than the index expects; reindex"
    );
    let source = lines[from..to].join("\n");

    // Parsed by the extractor that indexed it. Assuming Ruby here told every
    // Rust user their files had changed when they had not.
    let found = crate::index::units_at(path, source.as_bytes())
        .and_then(|blob| blob.units.first().and_then(|u| u.norm_hash));
    anyhow::ensure!(
        found == Some(norm_hash),
        "the file changed since it was indexed; reindex"
    );
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_states_read_from_the_counts() {
        let state = |summarized, summarizable| {
            Coverage {
                summarizable,
                summarized,
            }
            .state()
        };
        assert_eq!(state(0, 10), "none");
        assert_eq!(state(3, 10), "warming");
        assert_eq!(state(10, 10), "complete");
        // A scope with nothing summarizable is not "warming" forever.
        assert_eq!(state(0, 0), "none");
    }
}
