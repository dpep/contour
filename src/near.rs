//! The near-structural tier: bodies that are mostly, but not exactly, the same
//! shape.
//!
//! The exact tier is a group-by on one column and costs nothing. This one needs
//! a *distance*, and a hash is equal or it is not — so the distance is over
//! **subtree signatures**: the set of sub-shapes inside a normalized body,
//! collected on the way up during the fold that produces `norm_hash`
//! (`ruby::norm`). Similarity is Jaccard over those sets, and that number is
//! the confidence, because here the judgment really is graded (DEC-010).
//!
//! ## Why there is no pairwise scan
//!
//! rails holds ~36k Ruby bodies, which is ~650 million pairs. Comparing them
//! all is not a slow implementation of this feature, it is a different feature
//! nobody can run.
//!
//! Instead, candidates come from an **inverted index** on subtree hashes: two
//! bodies can only score above the threshold if they share a sub-shape, so
//! anything sharing none is provably below it and never compared. Two things
//! keep that index from degenerating:
//!
//! - a size floor on what enters a signature at all
//!   (`ruby::norm::MIN_SUBTREE_NODES`), because every method in a corpus
//!   contains a bare local read; and
//! - [`COMMON_SUBTREE_CAP`], which drops sub-shapes so widespread that they
//!   say nothing about any particular pair — a guard clause, an empty hash
//!   literal — and which would otherwise put one enormous bucket into the
//!   middle of the join.
//!
//! Measured on rails: see [`COMMON_SUBTREE_CAP`].

use crate::store::{Located, Store};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// A sub-shape appearing in more than this many bodies is dropped from the
/// index.
///
/// It is not evidence about a pair — everything has it — and it is what turns
/// candidate generation quadratic: a bucket of 4,000 bodies is 8 million pairs
/// on its own.
///
/// Measured on rails: 20,302 distinct Ruby bodies hold 197,441 distinct
/// sub-shapes, and exactly **4** of those exceed this cap. Dropping those four
/// takes candidate generation from 206,075,451 possible pairs down to 56,706
/// actually compared — a 3,600x reduction, and the whole near tier runs in
/// 0.7s. Reproduce with `contour dupes --near`, which prints these counts to
/// stderr on every run.
pub const COMMON_SUBTREE_CAP: usize = 64;

/// Jaccard at or above which two bodies are called near-structural.
///
/// **The first threshold in contour measured on its own corpus** rather than
/// inherited or guessed, because this is the first one with labels at both
/// edges. On rails:
///
/// | labeled | pairs | Jaccard |
/// | ------- | ----- | ------- |
/// | distinct (the DEC-017 `super` pairs) | 4 | 0.000, 0.000, 0.625, 0.667 |
/// | near-duplicate (copy-paste-then-tweak) | 4 | 0.905, 0.938, 1.000, 1.000 |
///
/// 0.80 sits in the middle of that 0.24-wide gap, 0.13 above the loudest
/// negative and 0.11 below the quietest positive. It also cuts the rails
/// report from 623 groups to 143, which is the difference between a list
/// somebody reads and a list somebody closes.
///
/// Still provisional: four labels on each side is a gap, not a distribution.
/// Widening the labeled set is what would move this number.
///
/// One property to know before reading any score: **Jaccard is harsher on a
/// small body.** An edit moves every subtree that contains it, so one added
/// call costs a short signature a third of itself (measured: 0.67 on an
/// eight-line pair) and a long one almost nothing (0.94 on the eighteen-line
/// `assert_queries_match` pair). A single threshold therefore finds long
/// near-duplicates more readily than short ones. That is a real limitation,
/// not a bug to route around with a size-dependent threshold — which would be
/// two thresholds to calibrate instead of one, on the strength of no evidence
/// at all.
pub const NEAR_THRESHOLD: f32 = 0.8;

/// Two bodies that are nearly the same shape.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pair {
    pub a: u64,
    pub b: u64,
    /// Jaccard over the two signatures. This *is* the confidence: a graded
    /// judgment reported as the measurement itself (DEC-010).
    pub similarity: f32,
    pub shared: usize,
}

/// How candidate generation behaved, so the scale story is auditable rather
/// than asserted.
#[derive(Debug, Default, serde::Serialize)]
pub struct Stats {
    pub bodies: usize,
    pub subtrees: usize,
    /// Sub-shapes dropped for appearing in more than [`COMMON_SUBTREE_CAP`]
    /// bodies.
    pub dropped_common: usize,
    /// Pairs actually compared. The number that says whether this scales.
    pub candidates: usize,
    /// Pairs a full scan would have compared.
    pub exhaustive: usize,
    /// Bodies in scope whose *language* has no subtree signature: Rust, whose
    /// hash is a token stream and admits no sub-shapes (DEC-012).
    pub uncovered_lang: usize,
    /// Bodies in scope whose every sub-shape fell below `norm`'s
    /// `MIN_SUBTREE_NODES`, leaving nothing to compare. A short body can be
    /// one — `super` plus an assignment has no subtree big enough to be
    /// evidence about any pair.
    ///
    /// Counted apart from [`Stats::uncovered_lang`] because they are different
    /// facts with different fixes, and a pure-Ruby repo told "the near tier is
    /// Ruby-only" has been handed a disclosure that is worse than none.
    pub uncovered_small: usize,
}

/// Every near-structural pair among the given bodies, with the work it took.
///
/// `signatures` is `norm_hash -> subtree hashes`, as the store keeps it.
pub fn pairs(signatures: &HashMap<u64, Vec<u64>>, threshold: f32) -> (Vec<Pair>, Stats) {
    let mut stats = Stats {
        bodies: signatures.len(),
        exhaustive: signatures.len().saturating_sub(1) * signatures.len() / 2,
        ..Stats::default()
    };

    // Inverted index: sub-shape -> the bodies containing it.
    let mut index: HashMap<u64, Vec<u64>> = HashMap::new();
    for (norm_hash, subtrees) in signatures {
        for subtree in subtrees {
            index.entry(*subtree).or_default().push(*norm_hash);
        }
    }
    stats.subtrees = index.len();

    // Count shared sub-shapes per candidate pair, skipping the buckets that
    // are too common to mean anything.
    let mut shared: HashMap<(u64, u64), usize> = HashMap::new();
    for bodies in index.values() {
        if bodies.len() > COMMON_SUBTREE_CAP {
            stats.dropped_common += 1;
            continue;
        }
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                // Ordered, so a pair is counted once however the walk found it.
                let key = if a < b { (*a, *b) } else { (*b, *a) };
                *shared.entry(key).or_insert(0) += 1;
            }
        }
    }
    stats.candidates = shared.len();

    let mut out: Vec<Pair> = shared
        .into_iter()
        .filter_map(|((a, b), shared)| {
            let (sa, sb) = (signatures.get(&a)?.len(), signatures.get(&b)?.len());
            let union = sa + sb - shared;
            let similarity = match union {
                0 => return None,
                union => shared as f32 / union as f32,
            };
            // An identical body is the *exact* tier's business, not this one.
            // Reporting it twice under two names makes a reader deduplicate by
            // hand.
            if similarity >= threshold && a != b {
                Some(Pair {
                    a,
                    b,
                    similarity,
                    shared,
                })
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|x, y| {
        y.similarity
            .total_cmp(&x.similarity)
            .then((x.a, x.b).cmp(&(y.a, y.b)))
    });
    (out, stats)
}

/// A near-structural neighbour, named the way a person reads it.
#[derive(Debug, serde::Serialize)]
pub struct Neighbor {
    pub path: String,
    pub id: String,
    pub line: u32,
    pub end_line: u32,
    pub similarity: f32,
}

/// Near-structural neighbours of one unit, within a checkout.
pub fn neighbors(
    store: &Store,
    root: &str,
    unit: &Located,
    threshold: f32,
    scope: Option<&str>,
) -> Result<Vec<Neighbor>> {
    let Some(norm_hash) = unit.unit.norm_hash else {
        return Ok(Vec::new());
    };
    let signatures = store.signatures()?;
    let Some(mine) = signatures.get(&norm_hash) else {
        return Ok(Vec::new());
    };
    let mine: HashSet<u64> = mine.iter().copied().collect();

    // Score only against bodies that share a sub-shape — same argument as
    // `pairs`, one body's worth of work.
    let mut scored: HashMap<u64, f32> = HashMap::new();
    for (other, subtrees) in &signatures {
        if *other == norm_hash {
            continue;
        }
        let shared = subtrees.iter().filter(|s| mine.contains(s)).count();
        if shared == 0 {
            continue;
        }
        let union = mine.len() + subtrees.len() - shared;
        let similarity = shared as f32 / union.max(1) as f32;
        if similarity >= threshold {
            scored.insert(*other, similarity);
        }
    }

    let mut out: Vec<Neighbor> = store
        .units(root)?
        .into_iter()
        .filter(|l| scope.is_none_or(|s| crate::paths::under(&l.path, s)))
        .filter_map(|l| {
            let similarity = *scored.get(&l.unit.norm_hash?)?;
            Some(Neighbor {
                id: l.unit.id(),
                path: l.path,
                line: l.unit.line,
                end_line: l.unit.end_line,
                similarity,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.similarity
            .total_cmp(&a.similarity)
            .then((&a.path, a.line).cmp(&(&b.path, b.line)))
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sigs(entries: &[(u64, &[u64])]) -> HashMap<u64, Vec<u64>> {
        entries.iter().map(|(k, v)| (*k, v.to_vec())).collect()
    }

    #[test]
    fn similarity_is_jaccard_over_the_signatures() {
        // A and B share 3 of 4; C shares one with A.
        let signatures = sigs(&[
            (1, &[10, 11, 12, 13]),
            (2, &[10, 11, 12, 99]),
            (3, &[10, 50, 51, 52]),
        ]);
        let (found, _) = pairs(&signatures, 0.5);
        assert_eq!(found.len(), 1, "only one pair clears 0.5");
        assert_eq!((found[0].a, found[0].b), (1, 2));
        // 3 shared of 5 in the union.
        assert!((found[0].similarity - 0.6).abs() < 1e-6);
        assert_eq!(found[0].shared, 3);
    }

    /// Bodies sharing no sub-shape are never compared — that is the whole
    /// scale argument, so it needs an assertion rather than a comment.
    #[test]
    fn bodies_sharing_nothing_are_not_candidates() {
        let signatures = sigs(&[(1, &[10, 11, 12]), (2, &[20, 21, 22])]);
        let (found, stats) = pairs(&signatures, 0.0);
        assert!(found.is_empty());
        assert_eq!(stats.candidates, 0, "no pair was even scored");
        assert_eq!(stats.exhaustive, 1, "a full scan would have compared one");
    }

    /// A sub-shape everything contains says nothing about any pair, and is
    /// what makes the index quadratic.
    #[test]
    fn an_over_common_subtree_is_dropped_from_the_index() {
        let everywhere = 7u64;
        let mut entries: Vec<(u64, Vec<u64>)> = (0..COMMON_SUBTREE_CAP as u64 + 2)
            .map(|i| (i, vec![everywhere, 1000 + i]))
            .collect();
        // Two bodies that genuinely match, through a sub-shape of their own.
        entries.push((900, vec![everywhere, 42, 43]));
        entries.push((901, vec![everywhere, 42, 43]));
        let signatures: HashMap<u64, Vec<u64>> = entries.into_iter().collect();

        let (found, stats) = pairs(&signatures, 0.5);
        assert_eq!(stats.dropped_common, 1);
        // Without the cap this would be thousands of candidate pairs.
        assert_eq!(stats.candidates, 1);
        assert_eq!((found[0].a, found[0].b), (900, 901));
    }

    /// An identical body belongs to the exact tier. Reporting it here too
    /// would make a reader deduplicate two tiers by hand.
    #[test]
    fn a_body_is_not_its_own_neighbour() {
        let signatures = sigs(&[(1, &[10, 11, 12, 13])]);
        assert!(pairs(&signatures, 0.0).0.is_empty());
    }
}
