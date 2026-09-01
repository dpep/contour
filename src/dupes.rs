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
    pub similarity: Option<f64>,
    pub members: Vec<Member>,
    /// Nodes in one member's normalized body — the size the payoff estimate
    /// is built from. `None` where `norm_hash` is.
    pub nodes: Option<u32>,
    /// Nodes that differ between the two bodies: **what consolidating would
    /// cost**, against `saves_nodes` as what it would buy. Near tier only — an
    /// exact group has nothing differing, and a field that is always zero
    /// teaches a reader to stop looking at it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub differing_nodes: Option<u32>,
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
    /// Why this group's consolidation may not be available. Filled by
    /// [`crate::constants::annotate`], and absent both when the group is fine
    /// and when the check could not run — the run-level stats say which.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caveat: Option<crate::constants::Caveat>,
    /// The population this group belongs to: a [`crate::paths::Class`] name
    /// where every copy agrees, `mixed` where they do not (DEC-022).
    ///
    /// It is what [`rank`] separates on. A group of specs is a real finding
    /// about test maintenance and a poor answer to "what should I consolidate
    /// in this app", so it is reported, tagged, and ranked after the app
    /// population rather than dropped or interleaved.
    pub class: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct Member {
    /// Absolute. See `paths::absolute` for why a record leaving the process
    /// does not carry a checkout-relative path.
    pub path: String,
    pub id: String,
    pub line: u32,
    pub end_line: u32,
    /// What kind of file this copy lives in (DEC-022). Per member rather than
    /// only per group, because in a mixed group *which* copy is the test one is
    /// the first thing a reader needs.
    pub class: crate::paths::Class,
    /// The namespace this copy is written in, `::`-joined and lexical. Carried
    /// rather than split back out of `id`, because whether the copies sit
    /// under *different* namespaces is what decides whether an unqualified
    /// constant in the body means the same thing in each (see
    /// [`crate::constants`]).
    pub owner: String,
}

/// Groups, plus what the path policy kept out of them.
///
/// One type for both tiers, so a caller that merges them merges the disclosure
/// too and cannot report half of it.
#[derive(Debug, Default, serde::Serialize)]
pub struct Found {
    pub groups: Vec<Group>,
    pub withheld: crate::paths::Withheld,
}

/// Group one checkout's units by normalized body.
///
/// `scope` is a checkout-relative path prefix; `None` means the whole
/// checkout. `min_lines` drops bodies too small for structural identity to
/// mean anything — see `cli::DEFAULT_MIN_LINES` for where the number comes
/// from. `classes` decides which path classes are reported (DEC-022).
pub fn find(
    store: &Store,
    root: &str,
    scope: Option<&str>,
    min_lines: u32,
    classes: &crate::paths::Classes,
) -> Result<Found> {
    let mut by_hash: std::collections::HashMap<u64, Vec<Located>> = Default::default();
    for located in candidates(store, root, scope, min_lines)? {
        by_hash
            .entry(located.unit.norm_hash.expect("filtered above"))
            .or_default()
            .push(located);
    }

    let mut found = Found::default();
    for (hash, members) in by_hash {
        if members.len() < 2 {
            continue;
        }
        let copies: Vec<Member> = members.iter().map(|l| member(root, l, classes)).collect();
        found.keep(
            Group {
                norm_hash: format!("{hash:016x}"),
                how: members[0].unit.lang.hash_tier(),
                lang: members[0].unit.lang,
                lines: members[0].unit.end_line + 1 - members[0].unit.line,
                similarity: None,
                nodes: members[0].unit.nodes,
                differing_nodes: None,
                saves_nodes: estimate(members[0].unit.nodes, members.len()),
                canonical: None,
                caveat: None,
                class: population(&copies),
                members: copies,
            },
            classes,
        );
    }

    rank(&mut found.groups);
    Ok(found)
}

fn member(root: &str, l: &Located, classes: &crate::paths::Classes) -> Member {
    Member {
        id: l.unit.id(),
        owner: l.unit.owner.clone(),
        class: classes.of_unit(&l.path, &l.unit),
        // Absolute from here on: a record leaving the process has to be
        // resolvable by a reader who is not standing in the checkout. Human
        // output shortens it back.
        path: crate::paths::absolute(root, &l.path),
        line: l.unit.line,
        end_line: l.unit.end_line,
    }
}

/// Two decimals, as an f64 — `crate::search::round2`'s argument, applied to
/// the other measurement that leaves the process.
fn round2(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}

/// The one population a group belongs to, or `mixed` where its copies
/// disagree. Not a [`crate::paths::Class`]: a file has one class, and a group
/// of files need not.
fn population(members: &[Member]) -> &'static str {
    let first = members[0].class;
    match members.iter().all(|m| m.class == first) {
        true => first.as_str(),
        false => "mixed",
    }
}

impl Found {
    /// Take one group, or withhold it — the single place the path policy is
    /// applied to a report, so no tier and no surface can forget it.
    ///
    /// A group is withheld only when **every** copy sits in an ignored path. A
    /// body that appears in both `vendor/` and `app/` is a finding about the
    /// app copy, and withholding it would hide the one duplication somebody
    /// can actually act on.
    fn keep(&mut self, group: Group, classes: &crate::paths::Classes) {
        match group.members.iter().any(|m| classes.reports(m.class)) {
            true => self.groups.push(group),
            false => self.withheld.add(group.members[0].class),
        }
    }
}

/// Biggest expected payoff first, app code before the populations that are
/// reported apart from it.
///
/// Called by every producer *and* by every caller that merges two tiers, so a
/// report is ordered whether or not it was assembled from one pass. Idempotent,
/// which is what makes that safe.
///
/// Test and fixture duplication is real maintenance signal and is reported
/// (DEC-022) — but interleaving it with app code buries the app findings under
/// a corpus's worth of specs, which is the complaint the ruling started from.
/// A mixed group ranks with app code, because that is the copy it is about.
pub fn rank(groups: &mut [Group]) {
    groups.sort_by_key(|g| {
        (
            !matches!(g.class, "app" | "mixed"),
            std::cmp::Reverse(g.saves_nodes),
            g.members[0].path.clone(),
            g.members[0].line,
        )
    });
}

/// Nodes an exact-tier consolidation would remove: the copies beyond the
/// first, whole.
///
/// The near tier no longer comes through here. It used to, with the Jaccard as
/// a discount standing in for "how much of the body is actually shared" — and
/// M11 turned that into a measurement (`near::Pair::shared_nodes`), so the
/// stand-in retired rather than being tuned.
///
/// A body with no node count scores zero and sorts last. That is the honest
/// answer rather than a fallback in some other unit: a report ordered by two
/// units is ordered by neither.
fn estimate(nodes: Option<u32>, copies: usize) -> u32 {
    nodes.unwrap_or(0) * copies.saturating_sub(1) as u32
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
    classes: &crate::paths::Classes,
) -> Result<(Found, crate::near::Stats)> {
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
    let signatures: std::collections::HashMap<u64, Vec<crate::core::Subtree>> = store
        .signatures()?
        .into_iter()
        .filter(|(norm_hash, _)| by_hash.contains_key(norm_hash))
        .collect();
    let (pairs, mut stats) = crate::near::pairs(&signatures, threshold);
    // `pairs` returns what it had rather than what it would have found, so the
    // refusal has to happen here or an abandoned run reports a short list as a
    // complete one.
    crate::cancel::current().check()?;
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

    let mut found = Found::default();
    for pair in pairs {
        // One representative per body: a near pair is about two *shapes*, and
        // listing every clone of each would bury the finding.
        let (Some(a), Some(b)) = (
            by_hash.get(&pair.a).and_then(|m| m.first()),
            by_hash.get(&pair.b).and_then(|m| m.first()),
        ) else {
            continue;
        };
        let copies: Vec<Member> = [a, b]
            .into_iter()
            .map(|l| member(root, l, classes))
            .collect();
        found.keep(
            Group {
                norm_hash: format!("{:016x}", pair.a),
                how: "near_structural",
                lang: a.unit.lang,
                lines: a.unit.end_line + 1 - a.unit.line,
                similarity: Some(round2(pair.similarity)),
                nodes: a.unit.nodes,
                differing_nodes: Some(pair.differing_nodes),
                // Measured, not estimated: `shared_nodes` IS the payoff, where
                // a body size discounted by a similarity ratio was a stand-in
                // for it (DEC-020). The two disagree most exactly where it
                // matters — a pair sharing one big chunk of a long body.
                saves_nodes: pair.shared_nodes,
                canonical: None,
                caveat: None,
                class: population(&copies),
                members: copies,
            },
            classes,
        );
    }
    rank(&mut found.groups);
    Ok((found, stats))
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

        let (_, stats) = find_near(&store, "/r", None, 4, 0.8, &Default::default()).unwrap();
        assert_eq!(stats.uncovered_small, 2, "the Ruby bodies, too small");
        assert_eq!(stats.uncovered_lang, 1, "the Rust body, wrong language");
    }

    /// The owner's stated intuitions about the ordering, as assertions. Each
    /// falls out of the arithmetic — there is no weight anywhere making them
    /// true, which is the property worth protecting.
    #[test]
    fn the_payoff_estimate_orders_by_what_consolidating_buys() {
        assert_eq!(estimate(Some(60), 2), 60, "one copy removed");
        assert_eq!(estimate(Some(60), 3), 120, "two copies removed");

        // An exact group beats a near pair of the same body size, because the
        // near pair only shares part of it — and a big near pair still beats a
        // small exact one. Both now fall out of a measurement rather than a
        // discount: a near group's payoff IS its `shared_nodes`.
        let exact = estimate(Some(100), 2);
        assert!(exact > 90, "a near pair of this size shares less than all");
        assert!(340 > exact, "a big near pair still beats a small exact one");

        // No node count is no estimate. Falling back to lines here would order
        // one report by two units, which is to order it by neither.
        assert_eq!(estimate(None, 5), 0);
    }
}
