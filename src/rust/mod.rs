//! Rust → [`Unit`]s. The second language, and the one contour dogfoods on.
//!
//! Same interface as [`crate::ruby`]: bytes in, units out. Everything
//! downstream — summaries, embeddings, search — never learns which produced
//! what, which is the DEC-012 seam paying off.
//!
//! **Normalization here is a degraded tier on purpose.** Ruby gets a
//! normalized AST through rwr's node tables; Rust gets a comment-stripped
//! token stream, disclosed as `token_hash` and never as `structural`. That
//! catches copy-paste and reformatting and nothing subtler — a renamed local
//! moves the hash, where in Ruby it would not. DEC-012 is explicit that full
//! parity is only worth building if dogfooding proves the need, because
//! chasing it is how a second language becomes a tarpit.
//!
//! **Known limit: a free function has no owner.** Rust derives a module path
//! from the *file* path (`src/rust/mod.rs` is `crate::rust`), and layer 1 is
//! forbidden a path by DEC-003 — that prohibition is what lets one blob at two
//! paths cost one parse. So `walk` here is named `walk`, not `rust::walk`, and
//! two `walk`s in different files are two units with one name. It costs
//! nothing for `dupes` or `search`, which print the path beside every hit; it
//! costs `contour similar walk` an ambiguity. The fix, if it earns itself, is
//! to compose the module prefix at the *file* layer where paths legally live,
//! never by handing this function one.

use crate::core::{Blob, Lang, Param, ParamKind, Unit};
use crate::hash::{FNV_OFFSET, SEP, fnv1a};
use tree_sitter::{Node, Parser};

/// Every callable in one Rust blob.
///
/// Structs, enums, traits, and constants are walked through but not emitted —
/// contour indexes callables (DEC-014), and a `mod` or `impl` exists here only
/// to supply the path that names the functions inside it.
pub fn units(src: &[u8]) -> Blob {
    let Some(tree) = parse(src) else {
        return Blob {
            lines: count_lines(src),
            parse_errors: 1,
            ..Blob::default()
        };
    };
    let mut units = Vec::new();
    walk(&tree.root_node(), src, "", &mut units);
    Blob {
        units,
        // Rust stays on the exact tier only. Shingling the token stream would
        // be the degraded analogue of a subtree signature, and DEC-012's rule
        // applies: build it if dogfooding shows the need, not for parity.
        signatures: std::collections::HashMap::new(),
        lines: count_lines(src),
        // tree-sitter recovers rather than failing, so an ERROR node is the
        // only signal that something did not parse.
        parse_errors: usize::from(tree.root_node().has_error()),
    }
}

fn parse(src: &[u8]) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    parser.parse(src, None)
}

fn count_lines(src: &[u8]) -> usize {
    match src.is_empty() {
        true => 0,
        false => src.iter().filter(|b| **b == b'\n').count() + 1,
    }
}

/// Collect callables, carrying the `::`-joined path they sit under.
fn walk(node: &Node<'_>, src: &[u8], owner: &str, out: &mut Vec<Unit>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            // `function_item` has a body. `function_signature_item` is a
            // bodyless trait declaration: a real name, but nothing to hash and
            // nothing to summarize, so it is a unit with no hash — the same
            // shape Ruby gives a macro-generated method.
            "function_item" | "function_signature_item" => {
                if let Some(unit) = function(&child, src, owner) {
                    out.push(unit);
                }
                // A `fn` inside a `fn` is its own callable.
                walk(&child, src, owner, out);
            }
            "impl_item" => {
                // An impl is not a callable; its type names the functions in
                // it. A trait impl (`impl Trait for Type`) is named by the
                // type, because that is where a reader looks for the body.
                let ty = field_text(&child, "type", src).map(|t| base_type(&t));
                walk(&child, src, &qualify(owner, ty.as_deref()), out);
            }
            "trait_item" | "mod_item" => {
                let name = field_text(&child, "name", src);
                walk(&child, src, &qualify(owner, name.as_deref()), out);
            }
            _ => walk(&child, src, owner, out),
        }
    }
}

fn function(node: &Node<'_>, src: &[u8], owner: &str) -> Option<Unit> {
    let name = field_text(node, "name", src)?;
    let params = node.child_by_field_name("parameters");
    let body = node.child_by_field_name("body");
    let folded = body.map(|body| token_hash(params.as_ref(), &body, src));
    Some(Unit {
        lang: Lang::Rust,
        name,
        owner: owner.to_string(),
        // Rust's own distinction, and the same fact `singleton` carries in
        // Ruby: calling an associated function needs no instance.
        singleton: !has_self(node),
        params: parameters(params.as_ref(), src),
        via: None,
        line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        // A bodyless signature has nothing to hash — the same treatment a
        // macro-generated Ruby method gets.
        norm_hash: folded.map(|(hash, _)| hash),
        nodes: folded.map(|(_, nodes)| nodes),
    })
}

/// Parameters, mapped onto the shared vocabulary.
///
/// Ruby's `Method#parameters` names are the neutral ones (DEC-014), and Rust
/// fits without strain: a plain parameter is `req`, and `self` is dropped
/// because `singleton` already records it. Rust has no optional, keyword, or
/// block parameters, so those variants simply never occur here — a language
/// using fewer of them is not a language the vocabulary fails.
fn parameters(params: Option<&Node<'_>>, src: &[u8]) -> Vec<Param> {
    let Some(params) = params else {
        return Vec::new();
    };
    let mut cursor = params.walk();
    params
        .children(&mut cursor)
        .filter(|child| child.kind() == "parameter")
        .filter_map(|child| {
            Some(Param {
                kind: ParamKind::Req,
                name: field_text(&child, "pattern", src)?,
            })
        })
        .collect()
}

fn has_self(node: &Node<'_>) -> bool {
    node.child_by_field_name("parameters")
        .is_some_and(|params| {
            let mut cursor = params.walk();
            params
                .children(&mut cursor)
                .any(|p| p.kind() == "self_parameter")
        })
}

/// The degraded structural hash: every token of the signature and body, with
/// comments dropped.
///
/// Whitespace and comments never appear because only leaf nodes are folded,
/// so a reformat or a re-comment collides. Nothing else is normalized — a
/// renamed local, a changed literal, and a different call all move the hash
/// equally. That is the honest limit of this tier, and why it is disclosed as
/// `token_hash`.
///
/// The function's own **name is excluded**, matching Ruby: two identically
/// bodied functions called `run` and `go` are the clone worth reporting.
fn token_hash(params: Option<&Node<'_>>, body: &Node<'_>, src: &[u8]) -> (u64, u32) {
    // Seeded with the language, so a Rust token stream can never collide with
    // a Ruby normalized body in the one `norm_hash` column they share.
    let mut hash = fnv1a(FNV_OFFSET, Lang::Rust.as_str().as_bytes());
    if let Some(params) = params {
        (hash, _) = fold_tokens(params, src, hash);
    }
    // Only the body's count, matching Ruby, where the parameter list is folded
    // into the hash but not into the size.
    fold_tokens(body, src, hash)
}

/// Fold every leaf token, and count the **named** nodes on the way.
///
/// Named only, because that is the closest tree-sitter comes to Prism's node
/// count: a CST counts every brace and comma, and `dupes` orders one report
/// across both languages. The two counts are still not calibrated against each
/// other — measured across rails and four Rust repos, Rust runs 5.6 named
/// nodes per line against Ruby's 3.4, so ~1.6x for the same body length —
/// so a mixed-language report's cross-language ordering is approximate, the
/// same disclosure `Lang::hash_tier` already makes about the hashes themselves.
fn fold_tokens(node: &Node<'_>, src: &[u8], mut hash: u64) -> (u64, u32) {
    let mut nodes = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind().contains("comment") {
            continue;
        }
        if child.is_named() {
            nodes += 1;
        }
        if child.child_count() == 0 {
            hash = fnv1a(hash, &src[child.byte_range()]);
            hash = fnv1a(hash, SEP);
        } else {
            let (folded, inner) = fold_tokens(&child, src, hash);
            hash = folded;
            nodes += inner;
        }
    }
    (hash, nodes)
}

fn field_text(node: &Node<'_>, field: &str, src: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    std::str::from_utf8(&src[child.byte_range()])
        .ok()
        .map(str::to_string)
}

fn qualify(owner: &str, name: Option<&str>) -> String {
    match (owner.is_empty(), name) {
        (_, None) => owner.to_string(),
        (true, Some(name)) => name.to_string(),
        (false, Some(name)) => format!("{owner}::{name}"),
    }
}

/// An impl's type, without generics or path: `Foo<T>` → `Foo`,
/// `a::b::Foo` → `Foo`. The short name is what a reader searches for.
fn base_type(ty: &str) -> String {
    let head = ty.split('<').next().unwrap_or(ty).trim();
    head.rsplit("::").next().unwrap_or(head).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> Vec<String> {
        units(src.as_bytes()).units.iter().map(Unit::id).collect()
    }

    fn hash(src: &str) -> u64 {
        units(src.as_bytes()).units[0].norm_hash.expect("a body")
    }

    #[test]
    fn names_functions_by_their_module_and_impl_path() {
        assert_eq!(
            ids("mod a { mod b { struct W; impl W { fn run(&self) {} } } }"),
            ["a::b::W::run"]
        );
        assert_eq!(ids("fn main() {}"), ["main"]);
        // A trait impl is named by the type: that is where the body lives.
        assert_eq!(
            ids("impl Display for Widget { fn fmt(&self) {} }"),
            ["Widget::fmt"]
        );
        // Generics and path qualifiers are not part of the name.
        assert_eq!(ids("impl<T> a::Widget<T> { fn run() {} }"), ["Widget::run"]);
    }

    #[test]
    fn records_whether_a_call_needs_an_instance() {
        let blob = units(b"impl W { fn new() -> W { W } fn run(&self, a: u8) {} }");
        assert!(
            blob.units[0].singleton,
            "an associated fn needs no instance"
        );
        assert!(!blob.units[1].singleton);
        // `self` is not a parameter — `singleton` already carries it.
        assert_eq!(blob.units[1].params.len(), 1);
        assert_eq!(blob.units[1].params[0].name, "a");
    }

    /// A trait's bodyless signature is a real name with nothing to hash — the
    /// same shape Ruby gives a macro-generated method.
    #[test]
    fn a_bodyless_signature_is_a_unit_with_no_hash() {
        let blob = units(b"trait T { fn run(&self); fn go(&self) {} }");
        assert_eq!(
            blob.units.iter().map(Unit::id).collect::<Vec<_>>(),
            ["T::run", "T::go"]
        );
        assert_eq!(blob.units[0].norm_hash, None);
        assert!(blob.units[1].norm_hash.is_some());
    }

    /// What the token tier does catch: presentation.
    #[test]
    fn formatting_and_comments_are_not_structure() {
        let base = hash("fn run(a: u8) -> u8 { let b = a + 1; b }");
        assert_eq!(base, hash("fn run(a:u8)->u8{let b=a+1;b}"));
        assert_eq!(
            base,
            hash("fn run(a: u8) -> u8 {\n    // explain\n    let b = a + 1;\n    b\n}")
        );
        assert_eq!(base, hash("fn go(a: u8) -> u8 { let b = a + 1; b }"));
    }

    /// And what it does not, which is the honest limit of the tier: a rename
    /// moves the hash here, where Ruby's normalization sees through it.
    #[test]
    fn a_rename_moves_the_token_hash_unlike_ruby() {
        assert_ne!(
            hash("fn run(a: u8) -> u8 { let b = a + 1; b }"),
            hash("fn run(x: u8) -> u8 { let y = x + 1; y }")
        );
    }

    #[test]
    fn changed_code_moves_the_hash() {
        let base = hash("fn run(a: u8) -> u8 { let b = a + 1; b }");
        for other in [
            "fn run(a: u8) -> u8 { let b = a + 2; b }",
            "fn run(a: u8) -> u8 { let b = a - 1; b }",
            "fn run(a: u16) -> u16 { let b = a + 1; b }",
            "fn run(a: u8, c: u8) -> u8 { let b = a + 1; b }",
        ] {
            assert_ne!(base, hash(other), "{other}");
        }
    }

    /// Two languages share one `norm_hash` column, so their hash spaces must
    /// not overlap — a Ruby body and a Rust body that happen to agree would
    /// otherwise be reported as clones of each other.
    #[test]
    fn a_rust_hash_is_seeded_with_its_language() {
        let rust = hash("fn run() { }");
        let (empty_seed, _) = fold_tokens(
            &parse(b"fn run() { }").unwrap().root_node(),
            b"fn run() { }",
            FNV_OFFSET,
        );
        assert_ne!(rust, empty_seed);
    }

    #[test]
    fn broken_source_yields_what_survived_and_admits_it() {
        let blob = units(b"fn ok() {}\nfn broken( {\n");
        assert!(blob.parse_errors > 0);
        assert!(blob.units.iter().any(|u| u.name == "ok"));
    }
}
