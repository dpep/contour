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
//! **The one principled exception to excluding the method's own name**
//! (DEC-017): a body containing `super` gets the enclosing def's
//! name folded in. `super` dispatches *by* that name, so two byte-identical
//! bodies ending in `super` run different code — `LocalCache#increment` and
//! `#decrement` in rails are exactly this. Reporting them as clones offers a
//! consolidation that is not available without metaprogramming, which is a
//! false lead rather than a harmless one. Everywhere else the name is
//! deliberately invisible; here it is structure.
//!
//! The result is an **on-disk key** — DEC-003 keys an LLM summary by it — so
//! everything folded in is chosen for stability across releases: the hash is a
//! vendored FNV-1a rather than `DefaultHasher`, and the node tag is a
//! generated variant name rather than a discriminant.

use super::extract::line_index::LineIndex;
use super::generated;
use super::node_tag::tag;
use crate::core::Subtree;
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
pub(crate) fn hashes(src: &[u8]) -> HashMap<(u32, u32), Normalized> {
    let parsed = ruby_prism::parse(src);
    let lines = LineIndex::new(src);
    let mut out = HashMap::new();
    walk(&parsed.node(), &lines, &mut out);
    out
}

fn walk(node: &Node<'_>, lines: &LineIndex, out: &mut HashMap<(u32, u32), Normalized>) {
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
fn def_hash(def: &ruby_prism::DefNode<'_>) -> Normalized {
    let mut fold = Fold {
        locals: HashMap::new(),
        enclosing: def.name().as_slice().to_vec(),
        subtrees: Vec::new(),
    };
    // Seeded with the language, so Ruby and Rust cannot collide into one
    // another's space in the single `norm_hash` column they share.
    let mut hash = fnv1a(FNV_OFFSET, crate::core::Lang::Ruby.as_str().as_bytes());
    hash = match def.parameters() {
        Some(params) => fnv1a(hash, &fold.node(&params.as_node()).0.to_le_bytes()),
        None => fnv1a(hash, b"()"),
    };
    let mut nodes = 0;
    hash = match def.body() {
        Some(body) => {
            let (folded, size) = fold.node(&body);
            nodes = size;
            fnv1a(hash, &folded.to_le_bytes())
        }
        None => fnv1a(hash, b"{}"),
    };
    // Sorted and deduplicated by shape: a signature is a *set*, and a measure
    // over it must not depend on traversal order or on a shape appearing
    // twice. The first occurrence's size and parent survive — the size is a
    // function of the shape either way, and see [`Subtree::parent`] for what
    // keeping one parent costs.
    fold.subtrees.sort_unstable_by_key(|s| s.hash);
    fold.subtrees.dedup_by_key(|s| s.hash);
    Normalized {
        hash,
        nodes,
        signature: fold.subtrees,
    }
}

/// A normalized body: its whole hash, and the hashes of the sub-shapes inside
/// it.
///
/// The signature is what makes *near*-structural comparison possible at all. A
/// whole-body hash answers only "identical or not"; the set of subtree hashes
/// admits a distance, and DEC-010 wants a graded judgment to carry a graded
/// number.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Normalized {
    pub(crate) hash: u64,
    /// Nodes in the normalized body. Free — the fold already counts them to
    /// apply `MIN_SUBTREE_NODES` — and a layout-invariant size measure, where
    /// lines are a formatting artefact.
    pub(crate) nodes: u32,
    pub(crate) signature: Vec<Subtree>,
}

/// Smallest subtree worth remembering, in nodes.
///
/// Below this every method in the corpus shares the same handful of shapes — a
/// bare local read, a one-argument call — and the signature stops
/// discriminating while the inverted index that uses it explodes. Measured on
/// rails; see `crate::near`.
const MIN_SUBTREE_NODES: u32 = 5;

struct Fold {
    /// Local name → ordinal, assigned on first sight. The traversal is a
    /// deterministic pre-order, and a read cannot precede its binding, so
    /// first sight *is* binding order.
    locals: HashMap<Vec<u8>, u32>,
    /// The name `super` would dispatch by here — the innermost enclosing
    /// `def`, which changes when the walk descends into a nested one.
    enclosing: Vec<u8>,
    /// Every subtree at or above [`MIN_SUBTREE_NODES`], in walk order.
    subtrees: Vec<Subtree>,
}

impl Fold {
    /// Hash one subtree, and its node count.
    ///
    /// A Merkle fold: a node's hash is a function of its own subtree and
    /// nothing above it, which is what lets any node's hash be compared
    /// against a node from another method. The previous rolling-stream fold
    /// could not do that — every value depended on everything walked before
    /// it.
    fn node(&mut self, node: &Node<'_>) -> (u64, u32) {
        let mut hash = fnv1a(FNV_OFFSET, tag(node).as_bytes());
        hash = fnv1a(hash, SEP);
        // `super` carries no atom naming what it calls, because the name is
        // the enclosing method's. Fold it in, or two bodies that dispatch
        // differently hash the same (DEC-017).
        if matches!(
            node,
            Node::SuperNode { .. } | Node::ForwardingSuperNode { .. }
        ) {
            hash = fnv1a(hash, b"^");
            hash = fnv1a(hash, &self.enclosing);
            hash = fnv1a(hash, SEP);
        }
        let rename = binds_local(node);
        for (i, atom) in generated::atoms(node).iter().enumerate() {
            match (i, rename, atom) {
                (0, true, Atom::Name(name)) => {
                    let ordinal = self.ordinal(name);
                    hash = fnv1a(hash, b"L");
                    hash = fnv1a(hash, &ordinal.to_le_bytes());
                }
                (_, _, Atom::Name(bytes)) => {
                    hash = fnv1a(hash, b"n");
                    hash = fnv1a(hash, bytes);
                }
                (_, _, Atom::Value(bytes)) => {
                    hash = fnv1a(hash, b"v");
                    hash = fnv1a(hash, bytes);
                }
                (_, _, Atom::Number(text)) => {
                    hash = fnv1a(hash, b"#");
                    hash = fnv1a(hash, text.as_bytes());
                }
            }
            hash = fnv1a(hash, SEP);
        }
        // The child count, so a node with two children can never fold like one
        // with a single child that happens to hash the same.
        let children = generated::children(node);
        hash = fnv1a(hash, &(children.len() as u32).to_le_bytes());
        // A nested `def` is what `super` inside it dispatches by, so the name
        // in scope changes for that subtree and is restored after it.
        let outer = match node.as_def_node() {
            Some(inner) => Some(std::mem::replace(
                &mut self.enclosing,
                inner.name().as_slice().to_vec(),
            )),
            None => None,
        };
        // Where this node's own recorded children start, so they can be
        // stamped with this node's hash once it is known. A parent's hash is
        // only complete after its children are folded into it, which is why
        // the link is written on the way back up.
        let recorded = self.subtrees.len();
        let mut size = 1;
        for child in &children {
            let (child_hash, child_size) = self.node(child);
            hash = fnv1a(hash, &child_hash.to_le_bytes());
            size += child_size;
        }
        if let Some(outer) = outer {
            self.enclosing = outer;
        }
        if size >= MIN_SUBTREE_NODES {
            for descendant in &mut self.subtrees[recorded..] {
                // Only the direct children: a grandchild was already stamped
                // by the child that contains it.
                if descendant.parent == 0 {
                    descendant.parent = hash;
                }
            }
            self.subtrees.push(Subtree {
                hash,
                nodes: size,
                parent: 0,
            });
        }
        (hash, size)
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
    use std::collections::HashSet;

    /// The hash of the sole method in a snippet.
    fn h(src: &str) -> u64 {
        let map = hashes(src.as_bytes());
        assert_eq!(map.len(), 1, "{src:?} should hold exactly one def");
        map.values().next().unwrap().hash
    }

    /// The signature of the sole method in a snippet, as shapes.
    fn sig(src: &str) -> Vec<u64> {
        subtrees(src).iter().map(|s| s.hash).collect()
    }

    /// The signature with the sizes and parent links the near tier measures by.
    fn subtrees(src: &str) -> Vec<Subtree> {
        hashes(src.as_bytes())
            .values()
            .next()
            .unwrap()
            .signature
            .clone()
    }

    /// The near tier measures in nodes, and every one of its sums leans on
    /// these three facts holding on real parsed Ruby rather than on a fixture.
    #[test]
    fn a_signature_carries_a_tree_it_can_be_measured_over() {
        let src = "def run(a)\n  b = a.fetch(:x)\n  c = b.map { |v| v.to_s }\n  \
                   log(c, b)\n  c\nend\n";
        let normalized = hashes(src.as_bytes());
        let body = normalized.values().next().unwrap();
        let sig = &body.signature;
        let sizes: HashMap<u64, u32> = sig.iter().map(|s| (s.hash, s.nodes)).collect();

        // Exactly one root, and it is the whole body — which is what lets the
        // near tier read a body's size out of its signature.
        let roots: Vec<&Subtree> = sig.iter().filter(|s| s.parent == 0).collect();
        assert_eq!(roots.len(), 1, "{sig:?}");
        assert_eq!(roots[0].nodes, body.nodes);

        for subtree in sig.iter().filter(|s| s.parent != 0) {
            // A parent is always recorded: it is strictly larger than its
            // child, so it clears the same floor.
            let parent = sizes
                .get(&subtree.parent)
                .unwrap_or_else(|| panic!("parent of {subtree:?} is not in {sig:?}"));
            assert!(*parent > subtree.nodes, "{subtree:?} against {parent}");
        }
        assert!(
            sig.iter().all(|s| s.nodes >= MIN_SUBTREE_NODES),
            "the floor is what keeps the index from degenerating: {sig:?}"
        );
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

    /// DEC-017, and the one place the method's own name is structure. rails
    /// has four live instances of this shape; `LocalCache#increment` and
    /// `#decrement` are byte-identical apart from the name and dispatch to
    /// different superclass methods.
    #[test]
    fn a_body_containing_super_is_named_by_it() {
        assert_ne!(
            h("def increment(a)\n  super\nend\n"),
            h("def decrement(a)\n  super\nend\n")
        );
        // Explicit `super(...)` dispatches by the same name and is separated
        // the same way.
        assert_ne!(
            h("def increment(a)\n  super(a)\nend\n"),
            h("def decrement(a)\n  super(a)\nend\n")
        );
        // Everywhere else the name stays invisible: without `super`, these two
        // are still the same method.
        assert_eq!(
            h("def increment(a)\n  store(a)\nend\n"),
            h("def decrement(a)\n  store(a)\nend\n")
        );
        // And the same name with the same body still agrees with itself.
        assert_eq!(
            h("def increment(a)\n  super\nend\n"),
            h("def increment(b)\n  super\nend\n")
        );
    }

    /// A nested `def` rebinds what `super` means, and the outer scope comes
    /// back afterwards.
    ///
    /// Isolated carefully: a def's *own* name is not an atom of its own hash
    /// (`def_hash` folds parameters and body, never the name), so renaming the
    /// outer def changes nothing except what its `super` dispatches by. An
    /// earlier version of this test renamed the *inner* def and passed without
    /// the rule at all, because a nested DefNode carries its name as an atom.
    #[test]
    fn a_nested_def_rebinds_what_super_dispatches_by() {
        let at = |src: String, line: u32| {
            hashes(src.as_bytes())
                .iter()
                .find(|((l, _), _)| *l == line)
                .expect("a def on that line")
                .1
                .hash
        };
        let nest = |outer: &str| format!("def {outer}\n  def b; super; end\n  super\nend\n");

        // The outer `super` follows the outer name.
        assert_ne!(at(nest("a"), 1), at(nest("z"), 1));
        // The inner one does not: it is `b` either way.
        assert_eq!(at(nest("a"), 2), at(nest("z"), 2));
    }

    /// The signature is what the near tier compares, so it has to behave like
    /// a fingerprint: a small edit keeps most of it, an unrelated body shares
    /// almost none of it.
    #[test]
    fn a_signature_survives_a_small_edit_and_not_a_different_method() {
        let jaccard = |a: &[u64], b: &[u64]| {
            let (a, b): (HashSet<_>, HashSet<_>) = (a.iter().collect(), b.iter().collect());
            a.intersection(&b).count() as f32 / a.union(&b).count() as f32
        };
        let base =
            sig("def run(rows)\n  rows.map { |r| fmt(r.name, r.size) }.compact.first\nend\n");
        // One extra call in the chain: still mostly the same shape.
        let edited =
            sig("def run(rows)\n  rows.map { |r| fmt(r.name, r.size) }.compact.uniq.first\nend\n");
        // Nothing in common but the language.
        let other =
            sig("def go(a, b)\n  raise Bad unless a\n  @cache[b] ||= Store.fetch(b)\nend\n");

        assert!(!base.is_empty(), "a body of this size has sub-shapes");
        assert!(
            jaccard(&base, &edited) > 0.5,
            "a small edit keeps most of it"
        );
        assert!(
            jaccard(&base, &other) < 0.1,
            "an unrelated body shares nothing"
        );
    }

    /// Trivia is kept out, or every method in a corpus would look alike and
    /// the inverted index behind the near tier would degenerate.
    #[test]
    fn a_tiny_body_contributes_no_sub_shapes() {
        assert!(sig("def run\n  @x\nend\n").is_empty());
        assert!(sig("def run(a)\n  a\nend\n").is_empty());
    }

    /// Frozen, because this is an on-disk key: an LLM summary is stored under
    /// it. If this moves, every summary in every database is orphaned — which
    /// is a schema bump, not a new expectation.
    #[test]
    fn the_hash_is_frozen() {
        assert_eq!(h("def run(a)\n  a.save\nend\n"), 0x38cf_b5f7_11c6_c8a7);
    }
}
