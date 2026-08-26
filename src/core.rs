//! The one noun contour is about.
//!
//! A [`Unit`] is **one callable span of source** — a Ruby method, later a Rust
//! `fn`. Everything the tool does is a lookup or a join over the chain of keys
//! a unit accumulates as it is progressively compressed:
//!
//! ```text
//! blob OID + name   where it is        (index, --symbols)
//! norm_hash         what shape it is   (dupes, similar/structural)
//! summary           what it means      (search, similar/semantic)
//! embedding         where it sits      (search ranking)
//! ```
//!
//! Each key is cheaper to compute and more specific than the next is expensive
//! and more general. `dupes` groups by the second, `search` ranks by the
//! fourth; nothing else is going on.
//!
//! This vocabulary is **language-neutral by construction** (DEC-012). Ruby's
//! own vocabulary — visibility stacks, Sorbet sigs, receiver shapes, ancestry
//! edges — lives in `ruby::facts` and stops at the `ruby::units` seam. A second
//! extractor produces `Unit`s directly and owes the rest of the engine nothing
//! else.

use serde::Serialize;

/// A git blob object id — 40 hex chars of SHA-1 over `blob <len>\0` + bytes.
///
/// Also the identity of a parse: same bytes, same OID, same units, forever
/// (DEC-003). Branch switches, rebases, and N worktrees cost nothing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Oid(pub String);

impl std::fmt::Display for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One callable span of source.
///
/// Deliberately *not* everything an extractor knows. Classes, constants,
/// ancestry edges, and call sites are all facts contour's Phase 1 has no
/// question for, and a record that carries them invites a query that depends
/// on them. Containers arrive in Phase 3 with a reason (DEC-013); calls arrive
/// when canonicality ranking needs reference counts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unit {
    pub name: String,
    /// The enclosing namespace as written, `::`-joined, empty at top level.
    /// Lexical, not resolved — `Foo::Bar`, never the class `Bar` actually
    /// reopens. Resolved ownership is a tree-layer question (Phase 3).
    pub owner: String,
    /// A method on the singleton: `def self.x`, or any `def` inside
    /// `class << self`. Decides `.` vs `#` in this unit's [`Unit::id`].
    pub singleton: bool,
    /// Ruby's own `Method#parameters` vocabulary, one per parameter. Kept
    /// because arity and keyword shape are part of what a caller sees, and a
    /// summarizer reads them as structural context (DEC-007).
    pub params: Vec<Param>,
    /// The macro that produced this unit (`attr_reader`, `scope`, `delegate`,
    /// …), or `None` for a literal definition. A macro-generated unit has no
    /// body, and so no [`Unit::norm_hash`] and nothing to summarize.
    pub via: Option<String>,
    pub line: u32,
    pub end_line: u32,
    /// Hash of the normalized body: locals renamed to ordinals, literals and
    /// layout collapsed. `None` when there is no body to hash. Populated in
    /// milestone 2.
    pub norm_hash: Option<u64>,
}

impl Unit {
    /// How a person names this unit, and what `contour similar` accepts:
    /// `Widget#save`, `Widget.find`, or a bare `helper` at top level.
    pub fn id(&self) -> String {
        let sep = if self.singleton { '.' } else { '#' };
        match self.owner.is_empty() {
            true => self.name.clone(),
            false => format!("{}{sep}{}", self.owner, self.name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Param {
    pub kind: ParamKind,
    pub name: String,
}

/// Ruby's `Method#parameters` vocabulary, which needs no glossary of ours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Req,
    Opt,
    Rest,
    Post,
    Keyreq,
    Key,
    Keyrest,
    Block,
    Nokey,
}

impl ParamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ParamKind::Req => "req",
            ParamKind::Opt => "opt",
            ParamKind::Rest => "rest",
            ParamKind::Post => "post",
            ParamKind::Keyreq => "keyreq",
            ParamKind::Key => "key",
            ParamKind::Keyrest => "keyrest",
            ParamKind::Block => "block",
            ParamKind::Nokey => "nokey",
        }
    }

    pub fn parse(s: &str) -> Option<ParamKind> {
        Some(match s {
            "req" => ParamKind::Req,
            "opt" => ParamKind::Opt,
            "rest" => ParamKind::Rest,
            "post" => ParamKind::Post,
            "keyreq" => ParamKind::Keyreq,
            "key" => ParamKind::Key,
            "keyrest" => ParamKind::Keyrest,
            "block" => ParamKind::Block,
            "nokey" => ParamKind::Nokey,
            _ => return None,
        })
    }
}

/// Everything one blob contributes, and how well it parsed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Blob {
    pub units: Vec<Unit>,
    pub lines: usize,
    /// Syntax errors the parser reported; the units above are what survived.
    pub parse_errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(owner: &str, name: &str, singleton: bool) -> Unit {
        Unit {
            name: name.into(),
            owner: owner.into(),
            singleton,
            params: Vec::new(),
            via: None,
            line: 1,
            end_line: 1,
            norm_hash: None,
        }
    }

    #[test]
    fn names_a_unit_the_way_a_person_writes_it() {
        assert_eq!(unit("Widget", "save", false).id(), "Widget#save");
        assert_eq!(unit("Widget", "find", true).id(), "Widget.find");
        assert_eq!(unit("A::B", "run", false).id(), "A::B#run");
        assert_eq!(unit("", "helper", false).id(), "helper");
    }
}
