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

    #[test]
    fn a_scope_stops_at_a_path_boundary() {
        assert!(under("app/models/widget.rb", "app/models"));
        assert!(under("app/models/widget.rb", "app/models/"));
        assert!(under("app/models/widget.rb", "app/models/widget.rb"));
        assert!(!under("app/models2/widget.rb", "app/models"));
        assert!(under("anything", "."));
    }
}
