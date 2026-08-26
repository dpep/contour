//! English queries, and nearest neighbours.
//!
//! Two halves, fused. The **lexical** half matches the query against a unit's
//! name; the **semantic** half matches it against the embedding of that unit's
//! summary. Reciprocal Rank Fusion merges them (gqls's `combine`, K=60): it is
//! scale-free, so a token-overlap count and a cosine need no common
//! normalization, and a unit strong in both rises above one strong in either.
//!
//! Every answer discloses what produced it (DEC-010) and how much of the
//! corpus could have participated (DEC-009). The semantic half only covers
//! summarized units, so a search over a half-summarized repo answers from what
//! exists and says so — rather than looking like the corpus is small.

use crate::embed::{Embedder, config_key, humanize, mrl, summary_text, text_key};
use crate::store::{Located, Store};
use crate::summary::Coverage;
use anyhow::{Result, bail};
use std::collections::HashMap;

/// Reciprocal Rank Fusion constant, from gqls. Large enough that the top few
/// ranks are not winner-take-all.
const RRF_K: f64 = 60.0;

/// The semantic half's weight in the fusion. Below 1.0 so an exact name match
/// keeps the lead: someone typing `unpaid_for` wants that method, not the one
/// whose summary is most poetic about invoices.
const SEMANTIC_WEIGHT: f64 = 0.7;

/// Cosine below which a hit is not an answer to anything — but only for an
/// embedder this was measured for.
///
/// Callers pass the floor in explicitly rather than having `search` read it,
/// so that `contour eval` can run with **no floor at all**. Calibrating a
/// threshold against results the threshold already filtered would only ever
/// show what survives today — a mistake the eval caught in its own harness.
///
/// gqls measured 0.40 on 256-dim MiniLM vectors over a 4602-record GraphQL
/// schema: the weakest real answer scored 0.526, the loudest nonsense 0.309.
/// That is **inherited, not calibrated for method summaries** — DEC-011 says
/// this number is settled by contour's own eval set, and until that exists it
/// is the best available guess rather than a measurement of this corpus.
///
/// The hash embedder gets **no floor at all**. It is lexical rather than
/// trained, so its cosine distribution is a different animal, and applying a
/// MiniLM-derived constant to it would be exactly the sort of borrowed number
/// that looks like evidence and is not. `CONTOUR_SEMANTIC_FLOOR` overrides
/// either way; `0` switches it off.
pub fn relevance_floor(kind: &str) -> f32 {
    if let Some(override_) = std::env::var("CONTOUR_SEMANTIC_FLOOR")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return override_;
    }
    match kind {
        "onnx" => 0.40,
        _ => 0.0,
    }
}

/// One search result.
#[derive(Debug, serde::Serialize)]
pub struct Hit {
    pub path: String,
    pub id: String,
    pub line: u32,
    /// Which half found it: `lexical`, `semantic`, or `both`.
    pub how: &'static str,
    /// The semantic half's measurement, when it contributed. Absent when the
    /// unit was found by name alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f32>,
    /// The fused rank score. Deliberately **not** called confidence: RRF is
    /// scale-free and its value means nothing outside this result set
    /// (DEC-010). `cosine` above is the measurement a reader should weigh.
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// A search answer plus everything needed to judge it.
#[derive(Debug, serde::Serialize)]
pub struct Answer {
    pub hits: Vec<Hit>,
    /// `complete` / `warming` / `none`, with the counts behind it.
    pub coverage: Coverage,
    pub coverage_state: &'static str,
    /// Which embedder answered — `hash` means the semantic half is lexical
    /// feature hashing, not a trained model, and should be read accordingly.
    pub embedder: &'static str,
    /// The absolute cosine floor applied, so a reader knows whether "no
    /// matches" means "nothing above the bar" or "no bar was set".
    pub floor: f32,
    /// Units the floor removed that would otherwise have ranked.
    ///
    /// The floor is inherited from another corpus and contour's own eval
    /// argues with it, so shipping it silently would be shipping an
    /// unfalsifiable constant. Reporting what it hid makes it auditable —
    /// trekr's `--include-excluded` move — and `--floor 0` shows the rest.
    pub withheld: usize,
}

/// Rank units in a checkout against an English query.
pub fn search(
    store: &mut Store,
    root: &str,
    scope: Option<&str>,
    query: &str,
    embedder: &dyn Embedder,
    limit: usize,
    floor: f32,
) -> Result<Answer> {
    let units = in_scope(store, root, scope)?;
    let summaries = summaries_for(store, &units, embedder)?;

    // Lexical: token overlap against the humanized name, best first.
    let mut lexical: Vec<(usize, f64)> = units
        .iter()
        .enumerate()
        .map(|(i, u)| (i, lexical_score(query, &u.unit.id())))
        .filter(|(_, score)| *score > 0.0)
        .collect();
    lexical.sort_by(|a, b| b.1.total_cmp(&a.1));

    // Semantic: cosine against each summary's vector, floored.
    let query_vec = mrl::compress_matryoshka_vector(&embedder.embed(query));
    let scored: Vec<(usize, f32)> = summaries
        .vectors
        .iter()
        .map(|(i, vec)| (*i, mrl::cosine_similarity(&query_vec, vec)))
        .filter(|(_, cosine)| *cosine > 0.0)
        .collect();
    let mut semantic: Vec<(usize, f32)> = scored
        .iter()
        .copied()
        .filter(|(_, cosine)| *cosine >= floor)
        .collect();
    let withheld = scored.len() - semantic.len();
    semantic.sort_by(|a, b| b.1.total_cmp(&a.1));

    let cosines: HashMap<usize, f32> = semantic.iter().copied().collect();
    let mut fused: HashMap<usize, (f64, bool, bool)> = HashMap::new();
    for (rank, (i, _)) in lexical.iter().enumerate() {
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + rank as f64 + 1.0);
        entry.1 = true;
    }
    for (rank, (i, _)) in semantic.iter().enumerate() {
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += SEMANTIC_WEIGHT / (RRF_K + rank as f64 + 1.0);
        entry.2 = true;
    }

    let mut ranked: Vec<(usize, f64, bool, bool)> = fused
        .into_iter()
        .map(|(i, (score, lex, sem))| (i, score, lex, sem))
        .collect();
    // Ties broken by location, so two runs of the same query agree.
    ranked.sort_by(|a, b| {
        b.1.total_cmp(&a.1).then_with(|| {
            (&units[a.0].path, units[a.0].unit.line).cmp(&(&units[b.0].path, units[b.0].unit.line))
        })
    });
    ranked.truncate(limit);

    let hits = ranked
        .into_iter()
        .map(|(i, score, lex, sem)| Hit {
            path: units[i].path.clone(),
            id: units[i].unit.id(),
            line: units[i].unit.line,
            how: match (lex, sem) {
                (true, true) => "both",
                (false, true) => "semantic",
                _ => "lexical",
            },
            cosine: cosines.get(&i).copied(),
            score,
            summary: summaries.text.get(&i).cloned(),
        })
        .collect();

    Ok(Answer {
        hits,
        coverage_state: summaries.coverage.state(),
        coverage: summaries.coverage,
        embedder: embedder.kind(),
        floor,
        withheld,
    })
}

/// One neighbour of a named unit, and how it was found.
#[derive(Debug, serde::Serialize)]
pub struct Neighbor {
    pub path: String,
    pub id: String,
    pub line: u32,
    /// The tier this was found by:
    ///
    /// - `structural` — an identical normalized body. A predicate, so it
    ///   carries evidence (`lines`) rather than a confidence (DEC-010).
    /// - `semantic` — a nearby summary embedding, with the cosine as its
    ///   confidence, because there the judgment really is graded.
    ///
    /// `near_structural` is named in the design and **not implemented**: it
    /// needs a structural *distance*, and a hash is equal or it is not.
    /// Building one is Phase 2's job, with a threshold the eval set settles.
    pub how: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Nearest neighbours of one named unit, structural tier first.
pub fn similar(
    store: &mut Store,
    root: &str,
    id: &str,
    embedder: &dyn Embedder,
    limit: usize,
) -> Result<Vec<Neighbor>> {
    let units = in_scope(store, root, None)?;
    let Some(target) = units.iter().position(|u| u.unit.id() == id) else {
        bail!("no unit named `{id}` in this checkout");
    };
    let summaries = summaries_for(store, &units, embedder)?;
    let here = &units[target];

    // Structural: an identical normalized body, at a different place.
    let mut out: Vec<Neighbor> = Vec::new();
    if let Some(norm_hash) = here.unit.norm_hash {
        for (i, other) in units.iter().enumerate() {
            let same_span = other.path == here.path && other.unit.line == here.unit.line;
            if i == target || same_span || other.unit.norm_hash != Some(norm_hash) {
                continue;
            }
            out.push(Neighbor {
                path: other.path.clone(),
                id: other.unit.id(),
                line: other.unit.line,
                how: "structural",
                confidence: None,
                lines: Some(other.unit.end_line + 1 - other.unit.line),
                summary: summaries.text.get(&i).cloned(),
            });
        }
    }

    // Semantic: nearest summaries, minus anything the structural tier already
    // claimed — a result reported twice under two tiers is a result a reader
    // has to de-duplicate by hand.
    if let Some(vec) = summaries.vectors.get(&target) {
        let claimed: std::collections::HashSet<(&str, u32)> =
            out.iter().map(|n| (n.path.as_str(), n.line)).collect();
        let mut scored: Vec<(usize, f32)> = summaries
            .vectors
            .iter()
            .filter(|(i, _)| **i != target)
            .filter(|(i, _)| !claimed.contains(&(units[**i].path.as_str(), units[**i].unit.line)))
            .map(|(i, other)| (*i, mrl::cosine_similarity(vec, other)))
            .filter(|(_, cosine)| *cosine >= relevance_floor(embedder.kind()))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (i, cosine) in scored.into_iter().take(limit) {
            out.push(Neighbor {
                path: units[i].path.clone(),
                id: units[i].unit.id(),
                line: units[i].unit.line,
                how: "semantic",
                // A graded judgment, so it gets a number — and the number is
                // the measurement itself, not a mapping of it.
                confidence: Some(cosine),
                lines: None,
                summary: summaries.text.get(&i).cloned(),
            });
        }
    }
    out.truncate(limit);
    Ok(out)
}

/// Token overlap between a query and a unit's humanized name.
///
/// Deliberately simple. RRF consumes a *ranking*, not a calibrated score, so
/// the elaborate fuzzy scorer gqls uses for GraphQL paths would be a borrowed
/// abstraction doing a job this does in twenty lines.
fn lexical_score(query: &str, id: &str) -> f64 {
    let name: Vec<String> = words(&humanize(id));
    if name.is_empty() {
        return 0.0;
    }
    let query = words(query);
    if query.is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    for term in &query {
        if name.iter().any(|w| w == term) {
            score += 1.0;
        } else if name.iter().any(|w| w.starts_with(term.as_str())) {
            // A prefix is real evidence but weaker: `pay` matching `payroll`
            // should not outrank `payroll` matching `payroll`.
            score += 0.5;
        }
    }
    score / query.len() as f64
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn in_scope(store: &Store, root: &str, scope: Option<&str>) -> Result<Vec<Located>> {
    Ok(store
        .units(root)?
        .into_iter()
        .filter(|l| scope.is_none_or(|s| crate::dupes::under(&l.path, s)))
        .collect())
}

/// The summaries and vectors for a set of units, embedding whatever is missing.
///
/// Embedding lazily here rather than in a command of its own keeps vectors in
/// step with summaries by construction: switching embedders re-embeds on the
/// next query instead of needing anyone to remember a step.
struct Indexed {
    /// Unit index → its summary vector.
    vectors: HashMap<usize, Vec<f32>>,
    /// Unit index → the human summary line, for display.
    text: HashMap<usize, String>,
    coverage: Coverage,
}

fn summaries_for(store: &mut Store, units: &[Located], embedder: &dyn Embedder) -> Result<Indexed> {
    let model = embedder.model().to_string();
    let config = config_key(embedder.kind(), &model);
    let have = store.vectors(config)?;
    let stored = store.all_summaries()?;

    let mut indexed = Indexed {
        vectors: HashMap::new(),
        text: HashMap::new(),
        coverage: Coverage::default(),
    };
    let mut missing: Vec<(u64, String)> = Vec::new();
    let mut pending: Vec<(usize, u64)> = Vec::new();

    for (i, located) in units.iter().enumerate() {
        let Some(norm_hash) = located.unit.norm_hash else {
            continue;
        };
        indexed.coverage.summarizable += 1;
        let ctx = crate::summary::Context::of(&located.unit).hash();
        let Some(summary) = stored.get(&(norm_hash, ctx)) else {
            continue;
        };
        indexed.coverage.summarized += 1;
        indexed.text.insert(i, summary.summary.clone());

        let text = summary_text(summary);
        let key = text_key(&text);
        match have.get(&key) {
            Some(vec) => {
                indexed.vectors.insert(i, vec.clone());
            }
            None => {
                missing.push((key, text));
                pending.push((i, key));
            }
        }
    }

    if !missing.is_empty() {
        // One vector per distinct text: identical summaries embed once.
        let mut fresh: HashMap<u64, Vec<f32>> = HashMap::new();
        for (key, text) in &missing {
            fresh
                .entry(*key)
                .or_insert_with(|| mrl::compress_matryoshka_vector(&embedder.embed(text)));
        }
        store.put_vectors(
            config,
            &fresh
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect::<Vec<_>>(),
        )?;
        for (i, key) in pending {
            if let Some(vec) = fresh.get(&key) {
                indexed.vectors.insert(i, vec.clone());
            }
        }
    }
    Ok(indexed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_match_beats_a_prefix_which_beats_nothing() {
        let exact = lexical_score("unpaid", "Invoice#unpaid_for");
        let prefix = lexical_score("unpa", "Invoice#unpaid_for");
        assert!(exact > prefix, "{exact} should beat {prefix}");
        assert!(prefix > 0.0);
        assert_eq!(lexical_score("payroll", "Invoice#unpaid_for"), 0.0);
    }

    /// Every query term has to pull its weight: a query that matches one of
    /// four words is a weaker match than one that matches one of one.
    #[test]
    fn the_score_is_a_fraction_of_the_query() {
        assert_eq!(lexical_score("unpaid", "Invoice#unpaid"), 1.0);
        assert!(lexical_score("unpaid invoices for a customer", "Invoice#unpaid") < 0.5);
    }

    /// The hash embedder is not a trained model, so a floor measured on MiniLM
    /// vectors would be a borrowed number pretending to be evidence.
    #[test]
    fn only_a_measured_embedder_gets_a_floor() {
        assert_eq!(relevance_floor("hash"), 0.0);
        assert!(relevance_floor("onnx") > 0.0);
    }
}
