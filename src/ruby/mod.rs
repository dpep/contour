//! Ruby → [`Unit`]s. The whole of Ruby stops here.
//!
//! One function is the module's interface (DEC-012): bytes in, units out.
//! Behind it sit ~2,750 vendored lines of Prism traversal and Rails macro
//! expansion, and none of that vocabulary — visibility stacks, Sorbet sigs,
//! receiver shapes, ancestry edges — crosses this line. A second language is a
//! second module producing the same `Vec<Unit>`.

mod extract;
mod facts;
mod generated;
mod node_tag;
mod norm;

use crate::core::{Blob, Lang, Param, ParamKind, Unit};

/// Every callable in one blob.
///
/// Classes, modules, and constants are extracted and dropped: contour's
/// Phase 1 asks no question about them, and a record that carries them invites
/// one that does (DEC-013 puts containers in Phase 3, with rollup summaries as
/// the reason).
pub fn units(src: &[u8]) -> Blob {
    let facts = extract::extract(src);
    let hashes = norm::hashes(src);
    let units = facts
        .defs
        .iter()
        .filter(|def| def.kind == facts::Kind::Method)
        .map(|def| Unit {
            lang: Lang::Ruby,
            name: def.name.clone(),
            owner: owner_path(&def.nesting),
            singleton: def.singleton,
            params: def.params.iter().map(param).collect(),
            via: def.via.clone(),
            line: def.pos.line,
            end_line: def.end_line,
            // Keyed by the position of the def's name, which is what the
            // extractor records. A macro-generated unit has no `def` node, so
            // it has no hash and nothing to summarize.
            norm_hash: hashes.get(&(def.pos.line, def.pos.col)).copied(),
        })
        .collect();
    Blob {
        units,
        lines: facts.lines,
        parse_errors: facts.parse_errors,
    }
}

/// Where the parser gave up, as `(line, col, message)`. Used by `--symbols` to
/// say that an outline is partial rather than to pretend a file is empty.
pub fn syntax_errors(src: &[u8]) -> Vec<(u32, u32, String)> {
    extract::syntax_errors(src)
}

/// A lexical scope stack (innermost first) as a person writes it.
///
/// `module A; module B` gives `["B", "A"]` → `A::B`; `module A::B` gives
/// `["A::B"]` → the same string. That the two spellings agree here is the
/// point — they are the same namespace, and only Ruby's constant *lookup*
/// distinguishes them, which is a tree-layer question contour does not ask.
fn owner_path(nesting: &[String]) -> String {
    nesting
        .iter()
        .rev()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("::")
}

fn param(p: &facts::Param) -> Param {
    // Both vocabularies are Ruby's `Method#parameters`; the duplication is the
    // language seam, not an accident. A `parse` round-trip keeps the two
    // enums honest without a match arm per variant.
    Param {
        kind: ParamKind::parse(p.kind.as_str()).unwrap_or(ParamKind::Req),
        name: p.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> Vec<String> {
        units(src.as_bytes())
            .units
            .iter()
            .map(Unit::id)
            .collect::<Vec<_>>()
    }

    #[test]
    fn names_every_callable_with_its_lexical_owner() {
        assert_eq!(
            ids("module A\n  class B\n    def save; end\n    def self.find; end\n  end\nend\n"),
            ["A::B#save", "A::B.find"]
        );
        assert_eq!(ids("module A::B\n  def run; end\nend\n"), ["A::B#run"]);
        assert_eq!(ids("def helper; end\n"), ["helper"]);
    }

    /// Rails macros define real methods, and a search that cannot find
    /// `Widget#name` because it came from `attr_reader` is a search with a
    /// hole in it. They carry `via` and no body.
    #[test]
    fn keeps_macro_generated_methods_and_says_what_made_them() {
        let blob = units(b"class Widget\n  attr_reader :name\n  def save; end\nend\n");
        let name = blob.units.iter().find(|u| u.name == "name").unwrap();
        assert_eq!(name.via.as_deref(), Some("attr_reader"));
        let save = blob.units.iter().find(|u| u.name == "save").unwrap();
        assert_eq!(save.via, None);
    }

    /// Classes and modules are extracted by the vendored layer and dropped
    /// here. Containers arrive in Phase 3 with rollup summaries as the reason.
    #[test]
    fn keeps_only_callables() {
        assert_eq!(
            ids("class Widget\n  MAX = 1\n  def save; end\nend\n"),
            ["Widget#save"]
        );
    }

    #[test]
    fn reads_parameters_in_rubys_own_vocabulary() {
        let blob = units(b"def run(a, b = 1, *rest, c:, d: 2, **opts, &blk); end\n");
        let kinds: Vec<&str> = blob.units[0]
            .params
            .iter()
            .map(|p| p.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            ["req", "opt", "rest", "keyreq", "key", "keyrest", "block"]
        );
    }

    /// A file that does not parse still yields what survived, and says how
    /// much it lost — an outline of a broken file beats no outline (DEC-010).
    #[test]
    fn survives_a_syntax_error_and_admits_it() {
        let blob = units(b"class Widget\n  def save; end\n  def broken(\nend\n");
        assert!(blob.parse_errors > 0);
        assert!(blob.units.iter().any(|u| u.name == "save"));
    }
}
