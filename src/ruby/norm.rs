//! Normalized structural hash: what a method *does*, with the spelling
//! removed.
//!
//! This is rwr's `node_eq` (`src/pattern/compare.rs`) turned inside out. Where
//! rwr compares two trees for structural equality — same variant, same atoms,
//! pairwise-equal children — contour folds the same three things into one
//! number, so equality becomes a lookup instead of a pairwise scan. The atom
//! table it folds is vendored whole from rwr, which is why `"x" == 'x'` and
//! `1_000 == 1000` come free and heredocs need no special case.
//!
//! On top of that, one normalization rwr does not do: **local variables are
//! renamed to ordinals in binding order**. A local is invisible outside the
//! method, so its spelling is not part of what the method does. Keyword
//! parameter names deliberately are *not* renamed — a caller writes
//! `f(force: true)`, so `force` is interface, not implementation.
//!
//! What survives normalization, and so still separates two methods: node
//! shape, method and constant names, instance-variable names, literal values,
//! keyword parameter names, and arity.
//!
//! The result is an **on-disk key** — DEC-003 keys an LLM summary by it — so
//! everything folded in is chosen for stability across releases: the hash is a
//! vendored FNV-1a rather than `DefaultHasher`, and the node tag is a
//! generated variant name rather than a discriminant.

use super::extract::line_index::LineIndex;
use super::generated;
use super::node_tag::tag;
use crate::hash::{FNV_OFFSET, SEP, fnv1a};
use ruby_prism::Node;
use std::collections::HashMap;

/// A name or value carried by a node but not represented as a child.
/// VENDORED from rwr `src/pattern/compare.rs`; `generated.rs` builds these.
///
/// `CallNode::name` is the motivating case: folding variant and children alone
/// would give `foo(a)` and `bar(a)` the same hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Atom {
    /// An identifier, by resolved bytes rather than by constant id.
    Name(Vec<u8>),
    /// A literal's **unescaped** value, so `"x"` equals `'x'` and heredoc
    /// bodies compare correctly with no heredoc-specific code.
    Value(Vec<u8>),
    /// Numeric values, normalised through their debug form so `1_000` equals
    /// `1000`.
    Number(String),
}

impl Atom {
    pub(crate) fn name(bytes: &[u8]) -> Self {
        Atom::Name(bytes.to_vec())
    }

    pub(crate) fn value(bytes: &[u8]) -> Self {
        Atom::Value(bytes.to_vec())
    }

    pub(crate) fn debug<T: std::fmt::Debug>(value: &T) -> Self {
        Atom::Number(format!("{value:?}"))
    }

    /// Prism's `Debug for Integer` prints the *pointer*, so debug-formatting
    /// an integer would make every literal hash differently -- silently, and
    /// in the direction that loses matches. Fold the digits instead.
    pub(crate) fn integer(value: &ruby_prism::Integer<'_>) -> Self {
        let (negative, digits) = value.to_u32_digits();
        Atom::Number(format!("{negative}:{digits:?}"))
    }
}

/// Every literal `def` in a blob, keyed by the `(line, col)` of its **name** —
/// which is exactly the position the extractor records as `Def::pos`, so the
/// two passes join without either knowing about the other.
///
/// This parses the source a second time. Measured: a cold full index of rails
/// went from 0.83 s to 1.10 s when this pass was added, and a warm one is
/// unchanged because it parses nothing at all. That 0.27 s buys the vendored
/// extractor staying a straight copy of trekr's. If it ever stops being worth
/// it, the fix is to hoist the parse into `super::units` and hand both passes
/// one `ParseResult` — an additive change to the vendored signature, not a
/// rewrite.
pub(crate) fn hashes(src: &[u8]) -> HashMap<(u32, u32), u64> {
    let parsed = ruby_prism::parse(src);
    let lines = LineIndex::new(src);
    let mut out = HashMap::new();
    walk(&parsed.node(), &lines, &mut out);
    out
}

fn walk(node: &Node<'_>, lines: &LineIndex, out: &mut HashMap<(u32, u32), u64>) {
    if let Some(def) = node.as_def_node() {
        let at = lines.pos(def.name_loc().start_offset());
        out.insert((at.line, at.col), def_hash(&def));
    }
    // Keep descending regardless: a `def` inside a `def` is its own method.
    for child in generated::children(node) {
        walk(&child, lines, out);
    }
}

/// One method's parameters and body, folded.
///
/// The method's **name is excluded**: two identically-bodied methods called
/// `save` and `persist` are the duplicate worth reporting, and giving them one
/// summary is the point of keying summaries this way (DEC-003). The parameter
/// list *is* included — arity and keyword shape are part of what a caller sees.
///
/// Not folded in: whether the method is a singleton. `def x` and `def self.x`
/// with the same body summarise the same way today. If milestone 3 finds that
/// the summarizer's structural context changes the answer, that context gets
/// its own hash beside this one rather than being smuggled into it.
fn def_hash(def: &ruby_prism::DefNode<'_>) -> u64 {
    let mut fold = Fold {
        hash: FNV_OFFSET,
        locals: HashMap::new(),
    };
    match def.parameters() {
        Some(params) => fold.node(&params.as_node()),
        None => fold.eat(b"()"),
    }
    match def.body() {
        Some(body) => fold.node(&body),
        None => fold.eat(b"{}"),
    }
    fold.hash
}

struct Fold {
    hash: u64,
    /// Local name → ordinal, assigned on first sight. The traversal is a
    /// deterministic pre-order, and a read cannot precede its binding, so
    /// first sight *is* binding order.
    locals: HashMap<Vec<u8>, u32>,
}

impl Fold {
    fn eat(&mut self, bytes: &[u8]) {
        self.hash = fnv1a(self.hash, bytes);
    }

    fn node(&mut self, node: &Node<'_>) {
        self.eat(tag(node).as_bytes());
        self.eat(SEP);
        let rename = binds_local(node);
        for (i, atom) in generated::atoms(node).iter().enumerate() {
            match (i, rename, atom) {
                (0, true, Atom::Name(name)) => {
                    let ordinal = self.ordinal(name);
                    self.eat(b"L");
                    self.eat(&ordinal.to_le_bytes());
                }
                (_, _, Atom::Name(bytes)) => {
                    self.eat(b"n");
                    self.eat(bytes);
                }
                (_, _, Atom::Value(bytes)) => {
                    self.eat(b"v");
                    self.eat(bytes);
                }
                (_, _, Atom::Number(text)) => {
                    self.eat(b"#");
                    self.eat(text.as_bytes());
                }
            }
            self.eat(SEP);
        }
        // The child count, so a pre-order walk is unambiguous: without it,
        // `f(g(a))` and `f(g, a)` can flatten to the same byte stream.
        let children = generated::children(node);
        self.eat(&(children.len() as u32).to_le_bytes());
        for child in &children {
            self.node(child);
        }
    }

    fn ordinal(&mut self, name: &[u8]) -> u32 {
        let next = self.locals.len() as u32;
        *self.locals.entry(name.to_vec()).or_insert(next)
    }
}

/// Nodes whose **first** atom names a local — a binding invisible outside the
/// method, so its spelling is not part of what the method does.
///
/// Keyword parameters (`RequiredKeywordParameterNode`,
/// `OptionalKeywordParameterNode`) are deliberately absent: a caller writes
/// `f(force: true)`, so `force` is the interface. Renaming it would make
/// `def price(currency:)` and `def weight(unit:)` collide, which is a false
/// clone, not a normalization.
///
/// Missing a node here is conservative: the hash is merely less normalized, so
/// two clones fail to collide. Adding one wrongly is not — it collapses
/// methods that differ.
fn binds_local(node: &Node<'_>) -> bool {
    matches!(
        node,
        Node::BlockLocalVariableNode { .. }
            | Node::BlockParameterNode { .. }
            | Node::KeywordRestParameterNode { .. }
            | Node::LocalVariableAndWriteNode { .. }
            | Node::LocalVariableOperatorWriteNode { .. }
            | Node::LocalVariableOrWriteNode { .. }
            | Node::LocalVariableReadNode { .. }
            | Node::LocalVariableTargetNode { .. }
            | Node::LocalVariableWriteNode { .. }
            | Node::OptionalParameterNode { .. }
            | Node::RequiredParameterNode { .. }
            | Node::RestParameterNode { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash of the sole method in a snippet.
    fn h(src: &str) -> u64 {
        let map = hashes(src.as_bytes());
        assert_eq!(map.len(), 1, "{src:?} should hold exactly one def");
        *map.values().next().unwrap()
    }

    #[test]
    fn layout_and_comments_are_not_structure() {
        let base = h("def run(a)\n  foo(a, 1)\nend\n");
        assert_eq!(base, h("def run(a)\n  foo(a,1)\nend\n"));
        assert_eq!(
            base,
            h("def run(a)\n  # explain\n  foo(\n    a,\n    1,\n  )\nend\n")
        );
    }

    #[test]
    fn quoting_and_numeric_spelling_are_not_structure() {
        assert_eq!(h(r#"def run; log("x"); end"#), h("def run; log('x'); end"));
        assert_eq!(h("def run; n(1000); end"), h("def run; n(1_000); end"));
    }

    /// The headline normalization: a rename is not a change.
    #[test]
    fn renaming_locals_and_positional_params_is_not_structure() {
        let base = h("def run(widget)\n  total = widget.price\n  total * 2\nend\n");
        assert_eq!(
            base,
            h("def run(thing)\n  sum = thing.price\n  sum * 2\nend\n")
        );
        // …and neither is renaming the method itself.
        assert_eq!(
            base,
            h("def go(thing)\n  sum = thing.price\n  sum * 2\nend\n")
        );
    }

    /// Ordinals are positional, so swapping which name holds which value is a
    /// real change even though the same two names appear.
    #[test]
    fn which_local_is_used_where_is_structure() {
        assert_ne!(
            h("def run(a, b)\n  a + b * a\nend\n"),
            h("def run(a, b)\n  b + a * a\nend\n")
        );
    }

    /// A keyword name is what the caller writes, so it is interface. This is
    /// the one place normalization deliberately stops.
    #[test]
    fn keyword_parameter_names_are_interface_not_implementation() {
        assert_ne!(
            h("def run(currency:)\n  fmt(currency)\nend\n"),
            h("def run(unit:)\n  fmt(unit)\nend\n")
        );
    }

    #[test]
    fn what_a_method_actually_does_is_structure() {
        let base = h("def run(a)\n  save(a)\nend\n");
        for (label, other) in [
            ("a different call", "def run(a)\n  destroy(a)\nend\n"),
            ("a different literal", "def run(a)\n  save(a, 2)\nend\n"),
            (
                "a different constant",
                "def run(a)\n  Widget.save(a)\nend\n",
            ),
            ("a different ivar", "def run(a)\n  save(@a)\nend\n"),
            ("a different arity", "def run(a, b)\n  save(a)\nend\n"),
            (
                "a guard added",
                "def run(a)\n  return unless a\n  save(a)\nend\n",
            ),
        ] {
            assert_ne!(base, h(other), "{label} must move the hash");
        }
    }

    /// Nesting must not flatten: a pre-order fold without the child count
    /// cannot tell these apart.
    #[test]
    fn nesting_is_structure() {
        assert_ne!(h("def r; f(g(a)); end"), h("def r; f(g, a); end"));
    }

    /// A `def` inside a `def` is its own method, and gets its own hash.
    #[test]
    fn finds_every_def_including_nested_ones() {
        let map = hashes(b"def outer\n  def inner; 1; end\nend\n");
        assert_eq!(map.len(), 2);
    }

    /// Frozen, because this is an on-disk key: an LLM summary is stored under
    /// it. If this moves, every summary in every database is orphaned — which
    /// is a schema bump, not a new expectation.
    #[test]
    fn the_hash_is_frozen() {
        assert_eq!(h("def run(a)\n  a.save\nend\n"), 0x0483_aa87_5270_736c);
    }
}
