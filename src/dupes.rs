//! Exact structural clones: units whose normalized bodies hash the same.
//!
//! This is the whole of the feature — a group-by over a column. That it is
//! free is the point: the hash exists because DEC-003 keys summaries by it, so
//! deduplicating the LLM's work and reporting duplicate code are the same
//! computation seen from two sides.
//!
//! Only the exact tier lives here. Near-structural and semantic duplicates
//! (Phase 2) need thresholds, and DEC-011 says thresholds come from eval
//! numbers rather than intuition.

use crate::store::{Located, Store};
use anyhow::Result;

/// A set of units that share one normalized body.
#[derive(Debug, serde::Serialize)]
pub struct Group {
    /// Hex, not a JSON number: a u64 past 2^53 does not survive a round trip
    /// through a JSON parser that stores numbers as doubles.
    pub norm_hash: String,
    /// How this group was found: `structural` for a normalized Ruby AST,
    /// `token_hash` for Rust's degraded token stream (DEC-012). A group never
    /// mixes the two, because each language seeds its own hash space.
    pub how: &'static str,
    /// The one language every member is written in — hash spaces are seeded
    /// per language, so a group cannot mix them.
    ///
    /// Carried rather than inferred from `how`, which cannot answer it for a
    /// near-structural group, and which would make every consumer that needs
    /// the language re-derive it from a string.
    pub lang: crate::core::Lang,
    /// Source lines spanned, `def` through `end`. The size disclosure that
    /// makes the floor honest: a reader can see whether a match is a real
    /// duplicate or two three-line accessors that happen to agree.
    pub lines: u32,
    /// Jaccard over subtree signatures, for the near tier only.
    ///
    /// Absent on an exact group on purpose: exact structural identity is a
    /// predicate and carries evidence (`lines`), where this is a graded
    /// judgment and carries the measurement itself (DEC-010).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
    pub members: Vec<Member>,
    /// Nodes in one member's normalized body — the size the payoff estimate
    /// is built from. `None` where `norm_hash` is.
    pub nodes: Option<u32>,
    /// **Estimated** AST nodes a consolidation would remove: the copies beyond
    /// the first, times the body's node count, discounted by how much shape
    /// they actually share.
    ///
    /// This is the report's ordering, because the rule `dupes` answers to is
    /// "would consolidating this reduce net complexity" — so the biggest
    /// reduction goes first. It needs no hand-tuned weight and encodes the
    /// right intuition on its own: an exact group beats a near one of the same
    /// size, while a big near pair still outranks a small exact one.
    ///
    /// ## Why nodes and not lines
    ///
    /// Measured on rails, and it is not a free choice: lines and nodes
    /// correlate at only **0.79** across 24,465 bodies, and the two orderings
    /// share **10 of their top 20** groups. Half the head of the report moves.
    ///
    /// Both proxies are distorted, in opposite directions. Lines overstate a
    /// heredoc — `ActiveJobAdapterTest#make_inline_test_file` is 83 lines and
    /// **5 nodes**, because a heredoc is one string node — and that inflates
    /// exactly the duplications least worth acting on. Nodes overstate a dense
    /// assertion list — `TimeExtCalculationsTest#test_advance` is 29 lines and
    /// 752 nodes. The heredoc error is the larger (16x against 8x) and it
    /// pushes the wrong things *up*.
    ///
    /// Two arguments settle it. **Layout invariance**: contour's whole premise
    /// is that a reformat is not a change (DEC-003), and an order that moves
    /// when somebody runs a formatter contradicts the system it belongs to.
    /// **Unit consistency**: the near tier's Jaccard is a fraction over
    /// node-space subtree sets, so `similarity × size` is only dimensionally
    /// coherent when size is nodes. Multiplying it by a line count produces a
    /// plausible number that means nothing — the exact failure DEC-010 exists
    /// to prevent.
    ///
    /// Lines are still printed beside it, because a person feels lines. What is
    /// ranked is what is shown, so the order can be argued with rather than
    /// trusted.
    pub saves_nodes: u32,
    /// Which member is likely the original, and on what basis. Filled by
    /// [`crate::canonical::annotate`] when asked for, and absent otherwise —
    /// the signals behind it are external process calls, so nothing computes
    /// them on a report that did not ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical: Option<crate::canonical::Canonical>,
}

#[derive(Debug, serde::Serialize)]
pub struct Member {
    /// Absolute. See `paths::absolute` for why a record leaving the process
    /// does not carry a checkout-relative path.
    pub path: String,
    pub id: String,
    pub line: u32,
    pub end_line: u32,
}

/// Group one checkout's units by normalized body.
///
/// `scope` is a checkout-relative path prefix; `None` means the whole
/// checkout. `min_lines` drops bodies too small for structural identity to
/// mean anything — see `cli::DEFAULT_MIN_LINES` for where the number comes
/// from.
pub fn find(store: &Store, root: &str, scope: Option<&str>, min_lines: u32) -> Result<Vec<Group>> {
    let mut candidates: Vec<Located> = store
        .units(root)?
        .into_iter()
        .filter(|l| l.unit.norm_hash.is_some())
        .filter(|l| l.unit.end_line + 1 - l.unit.line >= min_lines)
        .filter(|l| scope.is_none_or(|s| crate::paths::under(&l.path, s)))
        .collect();

    // One `def` can produce two units — `module_function` emits a private
    // instance method and a public singleton one from the same source. They
    // share a span, and reporting them as clones of each other would be the
    // tool arguing with itself. Grouping over distinct *spans* removes the
    // whole class rather than special-casing the macro.
    candidates.sort_by(|a, b| {
        (&a.path, a.unit.line, a.unit.norm_hash).cmp(&(&b.path, b.unit.line, b.unit.norm_hash))
    });
    candidates.dedup_by(|a, b| a.path == b.path && a.unit.line == b.unit.line);

    let mut by_hash: std::collections::HashMap<u64, Vec<Located>> = Default::default();
    for located in candidates {
        by_hash
            .entry(located.unit.norm_hash.expect("filtered above"))
            .or_default()
            .push(located);
    }

    let mut groups: Vec<Group> = by_hash
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(hash, members)| Group {
            norm_hash: format!("{hash:016x}"),
            how: members[0].unit.lang.hash_tier(),
            lang: members[0].unit.lang,
            lines: members[0].unit.end_line + 1 - members[0].unit.line,
            similarity: None,
            nodes: members[0].unit.nodes,
            saves_nodes: estimate(members[0].unit.nodes, members.len(), None),
            canonical: None,
            members: members
                .into_iter()
                .map(|l| Member {
                    id: l.unit.id(),
                    // Absolute from here on: a record leaving the process has
                    // to be resolvable by a reader who is not standing in the
                    // checkout. Human output shortens it back.
                    path: crate::paths::absolute(root, &l.path),
                    line: l.unit.line,
                    end_line: l.unit.end_line,
                })
                .collect(),
        })
        .collect();

    rank(&mut groups);
    Ok(groups)
}

/// Biggest expected payoff first.
///
/// Called by every producer *and* by every caller that merges two tiers, so a
/// report is ordered whether or not it was assembled from one pass. Idempotent,
/// which is what makes that safe.
pub fn rank(groups: &mut [Group]) {
    groups.sort_by_key(|g| {
        (
            std::cmp::Reverse(g.saves_nodes),
            g.members[0].path.clone(),
            g.members[0].line,
        )
    });
}

/// Nodes a consolidation would remove, estimated.
///
/// `similarity` is the near tier's Jaccard — already a shared-fraction
/// estimate over these very nodes, so it doubles as the discount and no second
/// constant is invented to serve as one. Absent for an exact group, where the
/// copies are the whole body.
///
/// A body with no node count scores zero and sorts last. That is the honest
/// answer rather than a fallback in some other unit: a report ordered by two
/// units is ordered by neither.
fn estimate(nodes: Option<u32>, copies: usize, similarity: Option<f32>) -> u32 {
    let beyond_the_first = copies.saturating_sub(1) as f32;
    (nodes.unwrap_or(0) as f32 * beyond_the_first * similarity.unwrap_or(1.0)).round() as u32
}

/// Near-structural pairs in a checkout, as groups of two.
///
/// A separate pass rather than a wider `find`, because the two tiers answer
/// different questions and merge only at the point of display. Jaccard is not
/// transitive, so a near result is inherently a *pair* — grouping them
/// transitively would assert a sameness nothing measured.
pub fn find_near(
    store: &Store,
    root: &str,
    scope: Option<&str>,
    min_lines: u32,
    threshold: f32,
) -> Result<(Vec<Group>, crate::near::Stats)> {
    let located = candidates(store, root, scope, min_lines)?;
    let mut by_hash: std::collections::HashMap<u64, Vec<Located>> = Default::default();
    for unit in located {
        by_hash
            .entry(unit.unit.norm_hash.expect("filtered"))
            .or_default()
            .push(unit);
    }

    // Only bodies actually present in this scope, so a pair cannot be reported
    // between two methods the caller cannot see.
    let signatures: std::collections::HashMap<u64, Vec<u64>> = store
        .signatures()?
        .into_iter()
        .filter(|(norm_hash, _)| by_hash.contains_key(norm_hash))
        .collect();
    let (pairs, mut stats) = crate::near::pairs(&signatures, threshold);
    // A body with no signature is uncovered for one of two reasons, and they
    // are not interchangeable: Rust has no sub-shapes at all (DEC-012), while
    // a Ruby body can simply be too small to hold one above the size floor.
    // Blaming the second on the first tells a pure-Ruby repo something false.
    for (norm_hash, members) in &by_hash {
        if signatures.contains_key(norm_hash) {
            continue;
        }
        match members[0].unit.lang {
            crate::core::Lang::Rust => stats.uncovered_lang += 1,
            crate::core::Lang::Ruby => stats.uncovered_small += 1,
        }
    }

    let groups = pairs
        .into_iter()
        .filter_map(|pair| {
            // One representative per body: a near pair is about two *shapes*,
            // and listing every clone of each would bury the finding.
            let a = by_hash.get(&pair.a)?.first()?;
            let b = by_hash.get(&pair.b)?.first()?;
            Some(Group {
                norm_hash: format!("{:016x}", pair.a),
                how: "near_structural",
                lang: a.unit.lang,
                lines: a.unit.end_line + 1 - a.unit.line,
                similarity: Some(pair.similarity),
                nodes: a.unit.nodes,
                saves_nodes: estimate(a.unit.nodes, 2, Some(pair.similarity)),
                canonical: None,
                members: [a, b]
                    .into_iter()
                    .map(|l| Member {
                        id: l.unit.id(),
                        path: crate::paths::absolute(root, &l.path),
                        line: l.unit.line,
                        end_line: l.unit.end_line,
                    })
                    .collect(),
            })
        })
        .collect();
    let mut groups: Vec<Group> = groups;
    rank(&mut groups);
    Ok((groups, stats))
}

/// Units in scope with a body worth comparing, deduplicated by span.
fn candidates(
    store: &Store,
    root: &str,
    scope: Option<&str>,
    min_lines: u32,
) -> Result<Vec<Located>> {
    let mut out: Vec<Located> = store
        .units(root)?
        .into_iter()
        .filter(|l| l.unit.norm_hash.is_some())
        .filter(|l| l.unit.end_line + 1 - l.unit.line >= min_lines)
        .filter(|l| scope.is_none_or(|s| crate::paths::under(&l.path, s)))
        .collect();
    out.sort_by(|a, b| {
        (&a.path, a.unit.line, a.unit.norm_hash).cmp(&(&b.path, b.unit.line, b.unit.norm_hash))
    });
    out.dedup_by(|a, b| a.path == b.path && a.unit.line == b.unit.line);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two bodies the near tier cannot consider, for two different reasons.
    /// Found by running the Claude skill against a pure-Ruby gem, where the
    /// only skipped body was Ruby and the message said Rust.
    #[test]
    fn a_body_the_near_tier_skips_is_blamed_on_the_right_thing() {
        let mut store = Store::open_in_memory().unwrap();
        // Two four-line bodies with no subtree big enough to enter a
        // signature. Two rather than one, so the counts differ and a test
        // with the buckets swapped fails — with one of each it passes both
        // ways, which is how this nearly shipped proving nothing.
        let ruby = "class A\n  def reset\n    super\n    @x = nil\n  end\n\
                    \n  def clear\n    foo\n    bar\n  end\nend\n";
        // Rust has no sub-shapes at any size — its hash is a token stream.
        let rust = "impl A {\n    fn reset(&mut self) {\n        self.x = 1;\n\
                    \n        self.y = 2;\n    }\n}\n";
        let (ruby_oid, ruby_blob) = (
            crate::scan::hash_blob(ruby.as_bytes()),
            crate::ruby::units(ruby.as_bytes()),
        );
        let (rust_oid, rust_blob) = (
            crate::scan::hash_blob(rust.as_bytes()),
            crate::rust::units(rust.as_bytes()),
        );
        let files: crate::scan::Files = [
            ("a.rb".to_string(), ruby_oid.clone()),
            ("a.rs".to_string(), rust_oid.clone()),
        ]
        .into_iter()
        .collect();
        store
            .write(
                "/r",
                &files,
                vec![(ruby_oid, ruby_blob), (rust_oid, rust_blob)],
            )
            .unwrap();

        let (_, stats) = find_near(&store, "/r", None, 4, 0.8).unwrap();
        assert_eq!(stats.uncovered_small, 2, "the Ruby bodies, too small");
        assert_eq!(stats.uncovered_lang, 1, "the Rust body, wrong language");
    }

    /// The owner's stated intuitions about the ordering, as assertions. Each
    /// falls out of the arithmetic — there is no weight anywhere making them
    /// true, which is the property worth protecting.
    #[test]
    fn the_payoff_estimate_orders_by_what_consolidating_buys() {
        assert_eq!(estimate(Some(60), 2, None), 60, "one copy removed");
        assert_eq!(estimate(Some(60), 3, None), 120, "two copies removed");
        assert_eq!(estimate(Some(100), 2, Some(0.9)), 90, "discounted by shape");

        let exact = estimate(Some(100), 2, None);
        assert!(
            exact > estimate(Some(100), 2, Some(0.9)),
            "exact beats near"
        );
        assert!(
            estimate(Some(400), 2, Some(0.85)) > exact,
            "a big near pair still beats a small exact one"
        );

        // No node count is no estimate. Falling back to lines here would order
        // one report by two units, which is to order it by neither.
        assert_eq!(estimate(None, 5, None), 0);
    }
}
