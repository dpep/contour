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
}

#[derive(Debug, serde::Serialize)]
pub struct Member {
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
        .filter(|l| scope.is_none_or(|s| under(&l.path, s)))
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
            lines: members[0].unit.end_line + 1 - members[0].unit.line,
            similarity: None,
            members: members
                .into_iter()
                .map(|l| Member {
                    id: l.unit.id(),
                    path: l.path,
                    line: l.unit.line,
                    end_line: l.unit.end_line,
                })
                .collect(),
        })
        .collect();

    // Biggest duplication first: lines × the copies beyond the original is the
    // code a reader would delete.
    groups.sort_by_key(|g| {
        (
            std::cmp::Reverse(g.lines * (g.members.len() as u32 - 1)),
            g.members[0].path.clone(),
            g.members[0].line,
        )
    });
    Ok(groups)
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
                lines: a.unit.end_line + 1 - a.unit.line,
                similarity: Some(pair.similarity),
                members: [a, b]
                    .into_iter()
                    .map(|l| Member {
                        id: l.unit.id(),
                        path: l.path.clone(),
                        line: l.unit.line,
                        end_line: l.unit.end_line,
                    })
                    .collect(),
            })
        })
        .collect();
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
        .filter(|l| scope.is_none_or(|s| under(&l.path, s)))
        .collect();
    out.sort_by(|a, b| {
        (&a.path, a.unit.line, a.unit.norm_hash).cmp(&(&b.path, b.unit.line, b.unit.norm_hash))
    });
    out.dedup_by(|a, b| a.path == b.path && a.unit.line == b.unit.line);
    Ok(out)
}

/// Is `path` inside the directory (or equal to the file) named by `prefix`?
/// Boundary-aware: `app/model` does not contain `app/models/widget.rb`.
///
/// Shared with `summary::fill`: a scope must mean the same thing to every
/// command that takes one, and two implementations would eventually disagree.
pub(crate) fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    path == prefix || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
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
                0,
            )
            .unwrap();

        let (_, stats) = find_near(&store, "/r", None, 4, 0.8).unwrap();
        assert_eq!(stats.uncovered_small, 2, "the Ruby bodies, too small");
        assert_eq!(stats.uncovered_lang, 1, "the Rust body, wrong language");
    }

    #[test]
    fn a_scope_stops_at_a_path_boundary() {
        assert!(under("app/models/widget.rb", "app/models"));
        assert!(under("app/models/widget.rb", "app/models/"));
        assert!(under("app/models/widget.rb", "app/models/widget.rb"));
        assert!(!under("app/models2/widget.rb", "app/models"));
        assert!(under("anything", "."));
    }
}
