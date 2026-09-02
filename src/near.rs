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
//! ## Two numbers, two jobs
//!
//! **Shapes decide; nodes price.** A pair is *called* near-structural by the
//! shape Jaccard above, and what consolidating it would buy is measured in
//! nodes — `shared_nodes` against `differing_nodes`, which is DEC-020's
//! arithmetic done with a measurement instead of a discount. The report ranks
//! by the second and thresholds on the first.
//!
//! That split is not the one M11 set out to build. The ratified direction was
//! to *replace* the shape measure with the node one, and the labels would not
//! support it:
//!
//! - On rails the shape measure wins clearly — but rails' near labels were
//!   drawn from this tier's own report at 0.80, which is the circularity
//!   `tests/eval/README.md` warns about.
//! - On discourse, whose labels were swept from 0.55 and read, the node measure
//!   wins modestly in the mid-range (at recall 0.50: 0.64 against 0.56).
//! - Merged, they are within noise of each other.
//!
//! The confound is not fixable by more careful arithmetic: **every positive in
//! both label sets was surfaced by the shape measure**, at 0.80 on rails and
//! 0.55 on discourse. A pair the node measure would find and the shape measure
//! scores below the sweep floor cannot be in the labels at all, so the
//! comparison can only ever confirm shapes. The label-sourcing rule applies to
//! a *measure* exactly as it applies to a threshold.
//!
//! One hypothesis was tested and rejected rather than assumed: that the node
//! cover was truncated by `MIN_SUBTREE_NODES`, so lowering the floor to 3 would
//! let it see the small unchanged material. Measured on discourse, it made the
//! node measure **worse** — recall 0.94 → 0.72, precision 0.61 → 0.54 — because
//! small shapes are shared by coincidence and inflate the cover. The floor
//! stays at 5.
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

use crate::core::Subtree;
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
/// **Recalibrated in M11 against both corpora at once**, by the sweep
/// `contour eval` now prints. 0.80 came from four labels on each side of one
/// corpus; this comes from 33 near labels and 20 distinct ones across rails and
/// discourse, including the 4–8 line band that 0.80 was known to be failing.
///
/// | threshold | precision | recall | short band |
/// | --------- | --------- | ------ | ---------- |
/// | 0.80 (was) | 0.68 (17/25) | 0.52 (17/33) | 2/13 |
/// | **0.70** | **0.71 (22/31)** | **0.67 (22/33)** | **6/13** |
/// | 0.65 | 0.67 (26/39) | 0.79 (26/33) | 8/13 |
/// | 0.60 | 0.65 (30/46) | 0.91 (30/33) | 11/13 |
///
/// 0.70 is the only move that improves **both** axes over 0.80, which is why it
/// is the one taken without an argument about how to trade them. The curve
/// below it is real and has no knee — but discourse's near labels were sourced
/// by sweeping this measure down to 0.55, so recall at 0.65 and below is partly
/// measuring the sourcing method rather than the tier (`tests/eval/README.md`).
/// Calibrating into that region would be the mistake DEC-011 exists to stop.
///
/// **The short band is improved, not fixed: 6 of 13.** M11 tried to fix it with
/// a change of measure — payoff over effort, node-denominated, the ratified
/// direction — and the labels could not show it winning. See the module header
/// and `docs/PLAN.md` for what was measured and what would settle it.
///
/// The property behind the band, unchanged: **Jaccard is harsher on a small
/// body.** An edit invalidates every sub-shape containing it, so one added call
/// costs a short signature a third of itself and a long one almost nothing. The
/// node counts on every [`Pair`] are what a reader should weigh against a ratio
/// that knows nothing about size.
pub const NEAR_THRESHOLD: f32 = 0.7;

/// Two bodies that are nearly the same shape, and what consolidating them
/// would buy against what it would cost.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pair {
    pub a: u64,
    pub b: u64,
    /// **The judgment**: Jaccard over the two signatures' sub-shapes, and the
    /// measurement itself rather than a mapping of it (DEC-010). Why this and
    /// not the node measure beside it is the whole of [`NEAR_THRESHOLD`]'s
    /// second half.
    pub similarity: f32,
    /// Nodes the two bodies share, counted once: **what consolidating buys.**
    /// Measured rather than estimated, which is what lets the report rank by
    /// the payoff itself instead of by a body size discounted by a ratio.
    pub shared_nodes: u32,
    /// Nodes that differ across both bodies: **what consolidating costs.** The
    /// other half of DEC-020's arithmetic, and the number that says whether a
    /// high similarity is worth acting on.
    pub differing_nodes: u32,
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
/// `signatures` is `norm_hash -> its sub-shapes`, as the store keeps it.
///
/// **Shapes generate the candidates; nodes make the judgment.** Sharing a
/// sub-shape is a cheap necessary condition and the inverted index below is
/// genuinely good at it; how *much* two bodies share is a question about nodes,
/// which is the half that moved.
pub fn pairs(signatures: &HashMap<u64, Vec<Subtree>>, threshold: f32) -> (Vec<Pair>, Stats) {
    let mut stats = Stats {
        bodies: signatures.len(),
        exhaustive: signatures.len().saturating_sub(1) * signatures.len() / 2,
        ..Stats::default()
    };

    // Inverted index: sub-shape -> the bodies containing it.
    let mut index: HashMap<u64, Vec<u64>> = HashMap::new();
    for (norm_hash, subtrees) in signatures {
        for subtree in subtrees {
            index.entry(subtree.hash).or_default().push(*norm_hash);
        }
    }
    stats.subtrees = index.len();

    // Count shared sub-shapes per candidate pair, skipping the buckets that
    // are too common to mean anything.
    //
    // The longest single-threaded stretch in the tool, and the one an
    // abandoned `dupes --near` used to run to completion regardless. Checked
    // per bucket rather than per pair: a bucket is bounded by
    // `COMMON_SUBTREE_CAP`, so the check is at most that far from the flag and
    // costs one relaxed load.
    let cancel = crate::cancel::current();
    let mut shared: HashMap<(u64, u64), usize> = HashMap::new();
    for bodies in index.values() {
        if cancel.cancelled() {
            return (Vec::new(), stats);
        }
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
        .take_while(|_| !cancel.cancelled())
        .filter_map(|((a, b), shapes)| {
            // An identical body is the *exact* tier's business, not this one.
            // Reporting it twice under two names makes a reader deduplicate by
            // hand.
            if a == b {
                return None;
            }
            let pair = score(a, b, signatures.get(&a)?, signatures.get(&b)?, shapes)?;
            (pair.similarity >= threshold).then_some(pair)
        })
        .collect();
    out.sort_by(|x, y| {
        y.similarity
            .total_cmp(&x.similarity)
            .then((x.a, x.b).cmp(&(y.a, y.b)))
    });
    (out, stats)
}

/// Score one candidate pair: what consolidating it buys, what it costs, and
/// the ratio between them.
fn score(a: u64, b: u64, sa: &[Subtree], sb: &[Subtree], shapes: usize) -> Option<Pair> {
    let hashes = |sig: &[Subtree]| -> HashSet<u64> { sig.iter().map(|s| s.hash).collect() };
    let (ha, hb) = (hashes(sa), hashes(sb));
    let shared: HashSet<u64> = ha.intersection(&hb).copied().collect();
    if shared.is_empty() {
        return None;
    }
    let (nodes_a, nodes_b) = (body_nodes(sa)?, body_nodes(sb)?);
    let cover_a = cover(sa, &shared);
    let cover_b = cover(sb, &shared);
    // The smaller cover, and never more than a body holds. The two can differ,
    // and either can overshoot, only through the multiplicity `Subtree::parent`
    // gives up on: a shape occurring twice under different parents keeps one
    // parent, so its nodes can be counted both on their own and inside a shared
    // container. Clamping is the honest floor rather than a correction — it can
    // only under-state how much two bodies share.
    let shared_nodes = cover_a.min(cover_b).min(nodes_a).min(nodes_b);
    Some(Pair {
        a,
        b,
        // `shapes` is the count the inverted index arrived at, so a sub-shape
        // dropped for being too common (`COMMON_SUBTREE_CAP`) is missing from
        // this numerator while both denominators hold it. The asymmetry
        // under-states a pair that shares a very common shape, it predates this
        // milestone, and every threshold ever calibrated — 0.80 then, 0.70 now
        // — was measured against it. Changing it is a recalibration, not a
        // tidy-up. The node cover above deliberately does not inherit it: it
        // counts what the two bodies actually share.
        similarity: shapes as f32 / (ha.len() + hb.len() - shapes).max(1) as f32,
        shared_nodes,
        differing_nodes: (nodes_a - shared_nodes) + (nodes_b - shared_nodes),
        shared: shapes,
    })
}

/// Nodes in the whole body: the one recorded sub-shape with no parent.
///
/// The signature describes itself, so nothing has to carry a body's size
/// alongside it. A body too small to record even its own root has no signature
/// at all and never reaches here (`Stats::uncovered_small`).
fn body_nodes(sig: &[Subtree]) -> Option<u32> {
    sig.iter().find(|s| s.parent == 0).map(|s| s.nodes)
}

/// Nodes covered by the shared sub-shapes, counted once.
///
/// A shared shape whose *parent* is also shared contributes nothing: its nodes
/// are already inside the parent's count. That test is the whole reason
/// `Subtree::parent` is stored, and it is what makes this a node count rather
/// than a shape count — the sum over every shared shape would multiply each
/// node by its depth.
fn cover(sig: &[Subtree], shared: &HashSet<u64>) -> u32 {
    sig.iter()
        .filter(|s| shared.contains(&s.hash) && !shared.contains(&s.parent))
        .map(|s| s.nodes)
        .sum()
}

/// The node measure, as a ratio: payoff against payoff-plus-effort.
///
/// **Not what decides a pair** — see [`NEAR_THRESHOLD`] for the measurement
/// that settled that, and for why. It is what `contour eval` sweeps as the
/// `nodes` row, and it is recoverable from the two counts a [`Pair`] carries,
/// so nothing has to store it.
pub fn node_ratio(shared_nodes: u32, differing_nodes: u32) -> f32 {
    match shared_nodes + differing_nodes {
        0 => 0.0,
        union => shared_nodes as f32 / union as f32,
    }
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

/// Near-structural neighbours of one unit, among `candidates`.
///
/// **The caller supplies the population.** This used to read the checkout's
/// units itself and filter them by `scope` — a second whole-checkout table
/// read, of exactly the rows its one caller was already holding, and on a
/// monorepo the single largest phase of a scoped `similar` was that read
/// happening twice (`--profile` says so in one line). Taking the units removes
/// the read, and with it the second place that spelled "is this path in
/// scope"; `search::similar` has already narrowed them.
pub fn neighbors(
    store: &Store,
    candidates: &[Located],
    unit: &Located,
    threshold: f32,
) -> Result<Vec<Neighbor>> {
    let Some(norm_hash) = unit.unit.norm_hash else {
        return Ok(Vec::new());
    };
    let signatures = store.signatures()?;
    let Some(mine) = signatures.get(&norm_hash) else {
        return Ok(Vec::new());
    };
    let shapes: HashSet<u64> = mine.iter().map(|s| s.hash).collect();

    // Score only against bodies that share a sub-shape — same argument as
    // `pairs`, one body's worth of work, and scored by the same measure so a
    // neighbour and a duplicate group cannot disagree about one pair.
    let mut scored: HashMap<u64, f32> = HashMap::new();
    for (other, subtrees) in &signatures {
        if *other == norm_hash {
            continue;
        }
        let count = subtrees.iter().filter(|s| shapes.contains(&s.hash)).count();
        if count == 0 {
            continue;
        }
        if let Some(pair) = score(norm_hash, *other, mine, subtrees, count)
            && pair.similarity >= threshold
        {
            scored.insert(*other, pair.similarity);
        }
    }

    let mut out: Vec<Neighbor> = candidates
        .iter()
        .filter_map(|l| {
            let similarity = *scored.get(&l.unit.norm_hash?)?;
            Some(Neighbor {
                id: l.unit.id(),
                path: l.path.clone(),
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

    /// A body as `(shape, nodes, parent)` rows, the way the store keeps it.
    /// The root carries parent 0 and the whole body's node count.
    fn body(rows: &[Row]) -> Vec<Subtree> {
        rows.iter()
            .map(|(hash, nodes, parent)| Subtree {
                hash: *hash,
                nodes: *nodes,
                parent: *parent,
            })
            .collect()
    }

    /// `(shape, nodes, parent)`, as the store keeps a row.
    type Row = (u64, u32, u64);

    fn sigs(entries: &[(u64, &[Row])]) -> HashMap<u64, Vec<Subtree>> {
        entries.iter().map(|(k, v)| (*k, body(v))).collect()
    }

    /// **The two numbers, and why there are two.**
    ///
    /// Two pairs, each with the same fraction of the body changed — 20% — but
    /// one short and flat, one long and deep. The shape measure calls the short
    /// one 0.33 and the long one 0.60, because a Merkle fold invalidates every
    /// shape above an edit and a deep body has more shapes for the survivors to
    /// hide in. The node measure calls both 0.67, which is what they both are.
    ///
    /// That gap is the short-body band, and it is why M11 tried to replace the
    /// judgment with the node ratio. The labels could not support the swap
    /// (module header), so the shape measure still decides and the node counts
    /// price the consolidation — but the disagreement is real, and this pins
    /// it so the next attempt starts from a number rather than from the
    /// argument.
    #[test]
    fn the_same_edit_scores_the_same_whatever_the_body_size() {
        // Short and flat: 20 nodes, one 3-node child replaced.
        let short = sigs(&[
            (
                1,
                &[(100, 20, 0), (110, 9, 100), (111, 7, 100), (120, 3, 100)],
            ),
            (
                2,
                &[(101, 20, 0), (110, 9, 101), (111, 7, 101), (121, 3, 101)],
            ),
        ]);
        // Long and deep: 200 nodes, one 39-node subtree replaced. The shapes
        // inside the survivors are what a shape count rewards it for.
        let long = sigs(&[
            (
                3,
                &[
                    (200, 200, 0),
                    (210, 120, 200),
                    (211, 40, 200),
                    (212, 39, 200),
                    (213, 60, 210),
                    (214, 59, 210),
                    (215, 20, 211),
                    (216, 19, 211),
                ],
            ),
            (
                4,
                &[
                    (201, 200, 0),
                    (210, 120, 201),
                    (211, 40, 201),
                    (218, 39, 201),
                    (213, 60, 210),
                    (214, 59, 210),
                    (215, 20, 211),
                    (216, 19, 211),
                ],
            ),
        ]);

        let one = |signatures: &HashMap<u64, Vec<Subtree>>| -> Pair {
            let (found, _) = pairs(signatures, 0.0);
            assert_eq!(found.len(), 1);
            found.into_iter().next().unwrap()
        };
        let (short, long) = (one(&short), one(&long));

        // What decides: the shape measure, which reads the same edit two ways.
        assert!((short.similarity - 0.333).abs() < 0.01, "{short:?}");
        assert!((long.similarity - 0.600).abs() < 0.01, "{long:?}");

        // What it costs and buys: nodes, which read it once.
        assert_eq!((short.shared_nodes, short.differing_nodes), (16, 8));
        assert_eq!((long.shared_nodes, long.differing_nodes), (160, 80));
        let ratio = |p: &Pair| node_ratio(p.shared_nodes, p.differing_nodes);
        assert!((ratio(&short) - 0.667).abs() < 0.01);
        assert!((ratio(&long) - 0.667).abs() < 0.01);
    }

    /// A shared shape inside another shared shape contributes nothing: its
    /// nodes are already counted. Without the parent test, a deep body's nodes
    /// are multiplied by their depth and the measure exceeds 1.
    #[test]
    fn a_shared_shape_inside_a_shared_shape_is_counted_once() {
        let signatures = sigs(&[
            (1, &[(10, 30, 0), (11, 20, 10), (12, 9, 11)]),
            (2, &[(20, 30, 0), (11, 20, 20), (12, 9, 11)]),
        ]);
        let (found, _) = pairs(&signatures, 0.0);
        // 20 nodes shared, not 29: the 9-node shape sits inside the 20-node one.
        assert_eq!(found[0].shared_nodes, 20);
        assert_eq!(found[0].differing_nodes, 20, "10 nodes over each body");
    }

    /// Bodies sharing no sub-shape are never compared — that is the whole
    /// scale argument, so it needs an assertion rather than a comment.
    #[test]
    fn bodies_sharing_nothing_are_not_candidates() {
        let signatures = sigs(&[
            (1, &[(10, 9, 0), (11, 5, 10), (12, 5, 10)]),
            (2, &[(20, 9, 0), (21, 5, 20), (22, 5, 20)]),
        ]);
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
        let mut entries: Vec<(u64, Vec<Subtree>)> = (0..COMMON_SUBTREE_CAP as u64 + 2)
            .map(|i| {
                (
                    i,
                    body(&[
                        (i + 5000, 12, 0),
                        (everywhere, 5, i + 5000),
                        (1000 + i, 6, i + 5000),
                    ]),
                )
            })
            .collect();
        // Two bodies that genuinely match, through sub-shapes of their own.
        let two = |root: u64| {
            body(&[
                (root, 18, 0),
                (everywhere, 5, root),
                (42, 6, root),
                (43, 6, root),
            ])
        };
        entries.push((900, two(9000)));
        entries.push((901, two(9001)));
        let signatures: HashMap<u64, Vec<Subtree>> = entries.into_iter().collect();

        // A low threshold on purpose: this test is about which pairs are
        // *generated*, and the two matching bodies each carry a root shape that
        // differs, so their shape Jaccard is 0.33.
        let (found, stats) = pairs(&signatures, 0.3);
        assert_eq!(stats.dropped_common, 1);
        // Without the cap this would be thousands of candidate pairs.
        assert_eq!(stats.candidates, 1);
        assert_eq!((found[0].a, found[0].b), (900, 901));
    }

    /// An identical body belongs to the exact tier. Reporting it here too
    /// would make a reader deduplicate two tiers by hand.
    #[test]
    fn a_body_is_not_its_own_neighbour() {
        let signatures = sigs(&[(1, &[(10, 20, 0), (11, 9, 10), (12, 9, 10)])]);
        assert!(pairs(&signatures, 0.0).0.is_empty());
    }
}
