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

/// A language contour can read.
///
/// Only extraction and normalization are language-specific (DEC-012);
/// everything expensive downstream operates on text and vectors. This tag
/// exists for three narrow reasons and no others: picking an extractor,
/// keeping one language's structural hashes out of another's space, and
/// rendering a unit's name the way that language's people write it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ruby,
    Rust,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Ruby => "ruby",
            Lang::Rust => "rust",
        }
    }

    pub fn parse(s: &str) -> Option<Lang> {
        match s {
            "ruby" => Some(Lang::Ruby),
            "rust" => Some(Lang::Rust),
            _ => None,
        }
    }

    /// How this language's structural hash was computed, as disclosed on every
    /// duplicate report (DEC-010, DEC-012).
    ///
    /// Ruby's is a normalized AST — locals renamed, literals and layout
    /// collapsed. Rust's is a **degraded tier by design**: a comment-stripped
    /// token stream, which catches exact-ish clones and nothing subtler. They
    /// must never both be called `structural`, because a reader would then
    /// believe a Rust group survived normalization it never saw.
    pub fn hash_tier(self) -> &'static str {
        match self {
            Lang::Ruby => "structural",
            Lang::Rust => "token_hash",
        }
    }
}

/// One callable span of source.
///
/// Deliberately *not* everything an extractor knows. Classes, constants,
/// ancestry edges, and call sites are all facts contour's Phase 1 has no
/// question for, and a record that carries them invites a query that depends
/// on them. Containers arrive in Phase 3 with a reason (DEC-013); calls arrive
/// when canonicality ranking needs reference counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub lang: Lang,
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
    /// Nodes in that normalized body — a size measure that survives a
    /// reformat, where `end_line - line` does not. `None` wherever
    /// `norm_hash` is.
    pub nodes: Option<u32>,
}

/// Serialized with its [`Unit::id`] alongside the fields it is derived from.
///
/// `id` is what every other surface prints and what `contour similar` accepts,
/// so a consumer that had to rebuild it from `owner`, `singleton` and `lang`
/// would be reimplementing a rule that already exists here — and would get
/// Rust wrong. Found by the MCP tests: `--symbols` was the one output without
/// it, which made the same concept two shapes depending on which call you made.
impl Serialize for Unit {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("Unit", 9)?;
        out.serialize_field("id", &self.id())?;
        out.serialize_field("lang", &self.lang)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("owner", &self.owner)?;
        out.serialize_field("singleton", &self.singleton)?;
        out.serialize_field("params", &self.params)?;
        out.serialize_field("via", &self.via)?;
        out.serialize_field("line", &self.line)?;
        out.serialize_field("end_line", &self.end_line)?;
        out.end()
    }
}

impl Unit {
    /// How a person names this unit, and what `contour similar` accepts.
    ///
    /// The one place the record is rendered in its own language's dialect:
    /// `Widget#save` and `Widget.find` in Ruby, `Widget::run` in Rust. Every
    /// other field here is neutral — this is presentation, and a Rust
    /// developer typing `Widget#run` is a Rust developer contour has confused.
    ///
    /// Rust does not spell the associated/instance distinction into the path,
    /// so `singleton` is carried but not shown there. It is still the same
    /// fact in both languages: whether calling this needs an instance.
    pub fn id(&self) -> String {
        id(self.lang, &self.owner, &self.name, self.singleton)
    }
}

/// The naming rule itself, so nothing has to restate it.
///
/// Restating it is how the fixture summarizer came to look up a Rust `fn`
/// under `Widget#total`, a name no surface prints and no person would write.
pub fn id(lang: Lang, owner: &str, name: &str, singleton: bool) -> String {
    if owner.is_empty() {
        return name.to_string();
    }
    match lang {
        Lang::Rust => format!("{owner}::{name}"),
        Lang::Ruby => {
            let sep = if singleton { '.' } else { '#' };
            format!("{owner}{sep}{name}")
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

/// One sub-shape inside a normalized body.
///
/// The hash alone answers "do these two bodies share this shape". The two
/// fields beside it are what make a **node-denominated** measure possible: a
/// shape's size is what consolidating it would buy, and its parent is what says
/// whether that size has already been counted by a larger shared shape. Without
/// them the near tier can only count *shapes*, and one edit invalidates every
/// shape above it — which is precisely why a short body scores badly
/// (`crate::near`).
///
/// Language-neutral like the rest of this module: a second extractor that can
/// produce sub-shapes produces these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Subtree {
    pub hash: u64,
    /// Nodes in this sub-shape. A function of the hash — the fold is a Merkle
    /// fold, so equal hashes are equal subtrees — but carried because nothing
    /// downstream could recompute it.
    pub nodes: u32,
    /// The hash of the shape immediately containing this one, or 0 at the body
    /// root. A recorded subtree's parent is always itself recorded: a parent is
    /// strictly larger than its child, so it clears the same size floor.
    ///
    /// One parent is kept for a shape occurring twice in one body under
    /// different parents. That makes the cover an **under**-count rather than a
    /// double-count, which is the direction to be wrong in.
    pub parent: u64,
}

/// Everything one blob contributes, and how well it parsed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Blob {
    pub units: Vec<Unit>,
    /// The sub-shapes inside each normalized body, keyed by its `norm_hash`.
    ///
    /// Keyed by the hash rather than carried on each [`Unit`] because it is a
    /// pure function of the body: a clone at ten call sites is one signature,
    /// stored once. This is what the near-structural tier compares.
    pub signatures: std::collections::HashMap<u64, Vec<Subtree>>,
    pub lines: usize,
    /// Syntax errors the parser reported; the units above are what survived.
    pub parse_errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(owner: &str, name: &str, singleton: bool) -> Unit {
        Unit {
            lang: Lang::Ruby,
            name: name.into(),
            owner: owner.into(),
            singleton,
            params: Vec::new(),
            via: None,
            line: 1,
            end_line: 1,
            norm_hash: None,
            nodes: None,
        }
    }

    /// Every surface prints the same handle, so a consumer never has to
    /// rebuild it — and cannot get Rust's `::` wrong by trying.
    #[test]
    fn a_serialized_unit_carries_the_name_people_use() {
        let json = serde_json::to_value(unit("Widget", "save", false)).unwrap();
        assert_eq!(json["id"], "Widget#save");
        assert_eq!(json["name"], "save");
        let rust = serde_json::to_value(Unit {
            lang: Lang::Rust,
            ..unit("Widget", "run", false)
        })
        .unwrap();
        assert_eq!(rust["id"], "Widget::run");
    }

    #[test]
    fn names_a_unit_the_way_a_person_writes_it() {
        assert_eq!(unit("Widget", "save", false).id(), "Widget#save");
        assert_eq!(unit("Widget", "find", true).id(), "Widget.find");
        assert_eq!(unit("A::B", "run", false).id(), "A::B#run");
        assert_eq!(unit("", "helper", false).id(), "helper");
    }

    /// Rust spells the same record its own way. The distinction `singleton`
    /// carries is real in both languages; only Ruby writes it into the name.
    #[test]
    fn a_rust_unit_is_named_the_way_rust_writes_it() {
        let rust = |owner: &str, name: &str, singleton: bool| Unit {
            lang: Lang::Rust,
            ..unit(owner, name, singleton)
        };
        assert_eq!(rust("Widget", "run", false).id(), "Widget::run");
        assert_eq!(rust("Widget", "new", true).id(), "Widget::new");
        assert_eq!(rust("a::b::Widget", "run", false).id(), "a::b::Widget::run");
        assert_eq!(rust("", "main", true).id(), "main");
    }

    #[test]
    fn the_two_hash_tiers_are_never_confused() {
        assert_eq!(Lang::Ruby.hash_tier(), "structural");
        assert_eq!(Lang::Rust.hash_tier(), "token_hash");
    }
}
