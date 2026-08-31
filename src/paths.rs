//! What contour knows about a path: how to render it, and what kind of file it
//! names.
//!
//! Rendering lives here so no output surface can quietly forget it. Scoping
//! lives here because a path prefix has to mean one thing to every command
//! that takes one. And [`Class`] lives here because DEC-021 says path
//! knowledge belongs at the file layer — not in the extractor, and not in the
//! blob.
//!
//! Not in `scan`, which owns the other path question (`language`), because
//! `scan` is vendored from trekr and stays close enough to upstream that a
//! sync is a re-copy (DEC-002).

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::path::Path;

/// A path as a person should read it: `$HOME` shown as `~`.
///
/// **Display only.** `--json` and `--ndjson` keep absolute paths, because a
/// machine consumer that has to expand `~` is one that will forget to.
pub fn pretty(path: &str) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_string();
    };
    let home = home.to_string_lossy();
    let home = home.strip_suffix('/').unwrap_or(&home);
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    // Boundary-aware: a home of `/Users/dan` must not claim `/Users/danger/x`,
    // which a bare `strip_prefix` would.
    match path.len() > home.len()
        && path.starts_with(home)
        && path.as_bytes().get(home.len()) == Some(&b'/')
    {
        true => format!("~{}", &path[home.len()..]),
        false => path.to_string(),
    }
}

/// A checkout-relative path made absolute, for a machine consumer.
///
/// JSON carries absolute paths because the consumer is not standing anywhere:
/// a `dupes` result saying `app/models/x.rb` is unresolvable without knowing
/// which of the machine's checkouts it came from.
pub fn absolute(root: &str, path: &str) -> String {
    format!("{}/{path}", root.trim_end_matches('/'))
}

/// The inverse, for human output: a person *is* standing in the checkout, and
/// a full path on every line is noise around the part they need.
pub fn within<'a>(root: &str, path: &'a str) -> &'a str {
    let root = root.trim_end_matches('/');
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// Is `path` inside the directory (or equal to the file) named by `prefix`?
/// Boundary-aware: `app/model` does not contain `app/models/widget.rb`.
///
/// One implementation, because a scope must mean the same thing to `dupes`,
/// `search`, `similar` and `summarize` — two would eventually disagree.
pub fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    path == prefix || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

/// What kind of file a path names (DEC-022).
///
/// A **pure function of the path string**, never of the bytes. `scan::language`
/// makes the same call for the same reason — sniffing content would mean
/// reading every file in the repo to answer a question the layout already
/// answers — and it is what keeps this at the file layer: the same blob
/// vendored into one repo and authored in another is one parse and two
/// classifications (DEC-021).
///
/// The consequence to know: a fact that is not in the path is not visible
/// here. A Rust `#[cfg(test)] mod tests` sits in an app file, and nothing about
/// `src/lib.rs` says otherwise — so [`Classes::of_unit`] takes the unit as well
/// as the path, and is what ranking asks. This layer stays about files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// The code the repository is *for*. Everything not claimed below.
    App,
    /// Specs, tests, and their helpers. Reported, and reported **apart**:
    /// duplication in shared examples is real maintenance signal, and "just
    /// ignore tests" is the tempting wrong answer (DEC-022).
    Test,
    /// Sample inputs a test reads: fixture corpora, testbed trees. Included
    /// like tests, and a different population again — a corpus of deliberately
    /// similar files is data, not code somebody maintains.
    Fixture,
    /// Schema migrations. Frozen history: consolidating one is not a
    /// consolidation, it is a rewrite of the past, so under DEC-020 they are
    /// not duplicates at all.
    Migration,
    /// Machine-written code. Nobody consolidates it; it is regenerated.
    Generated,
    /// Somebody else's code, copied in. Nobody consolidates it either; it is
    /// re-copied.
    Vendored,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::App => "app",
            Class::Test => "test",
            Class::Fixture => "fixture",
            Class::Migration => "migration",
            Class::Generated => "generated",
            Class::Vendored => "vendored",
        }
    }

    pub fn parse(s: &str) -> Option<Class> {
        CLASSES.into_iter().find(|c| c.as_str() == s)
    }

    /// Whether a report shows this class by default, as the owner ruled
    /// (DEC-022). Every default is disclosed on every run and overridable —
    /// silently withholding a finding would be a worse failure than the one
    /// this fixes.
    pub fn reported(self) -> bool {
        !matches!(self, Class::Migration | Class::Generated | Class::Vendored)
    }

    /// Whether this is the population a reader is asking about. The rest are
    /// ranked after it rather than interleaved with it.
    pub fn is_app(self) -> bool {
        matches!(self, Class::App)
    }
}

/// Every class, for the vocabulary a config error prints.
pub const CLASSES: [Class; 6] = [
    Class::App,
    Class::Test,
    Class::Fixture,
    Class::Migration,
    Class::Generated,
    Class::Vendored,
];

/// The Rust module a file declares, from its path alone.
///
/// **The gap this closes has stood since Phase 1.5** (DEC-021's sibling): a
/// top-level `fn` has no lexical owner, so rq's five language plugins each
/// contributed an identically-named `tests::find` and nothing could tell them
/// apart — not `similar`, which refused as ambiguous; not `search`, which ranked
/// five copies of one answer; and not `store_summary`, where guessing the wrong
/// one costs a session's tokens. The module prefix that disambiguates them is
/// **in the path**, which is exactly why the extractor could never reach it and
/// why this lives here.
///
/// A pure function of the path string, like [`Class`] and for the same reason:
/// no reindex, and the same answer from `--symbols` on a file contour has never
/// seen as from the index.
///
/// The crate root and a `mod.rs` name no module of their own, so
/// `src/lib.rs` has none and `src/store/mod.rs` is `store`. Everything after the
/// last `src`, `tests`, `benches` or `examples` segment is the path; a file with
/// none of those keeps its stem, which is the honest answer for a layout this
/// rule does not recognise.
pub fn rust_module(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let after = match segments
        .iter()
        .rposition(|s| matches!(*s, "src" | "tests" | "benches" | "examples"))
    {
        Some(root) => &segments[root + 1..],
        // No recognised root: the file names itself and nothing more. Taking
        // the whole path would put a person's home directory in a unit's id.
        None => &segments[segments.len().checked_sub(1)?..],
    };
    let (last, parents) = after.split_last()?;
    let last = last.strip_suffix(".rs").unwrap_or(last);
    // Only the trailing run of directories that could *be* module names. A
    // module path is made of module names, and `tests/testbed/006-rust-names/`
    // is a directory of fixtures — taking it would put `006-rust-names` in a
    // unit's id, and make the answer depend on how far up the caller happened
    // to be standing when they named the file.
    let named = parents.len()
        - parents
            .iter()
            .rev()
            .take_while(|s| is_module_name(s))
            .count();
    let mut module: Vec<&str> = parents[named..].to_vec();
    if !matches!(last, "mod" | "lib" | "main") && is_module_name(last) {
        module.push(last);
    } else if !matches!(last, "mod" | "lib" | "main") {
        // A file whose own stem is not a module name names nothing; whatever
        // its directories say is about somewhere else.
        return None;
    }
    match module.is_empty() {
        true => None,
        false => Some(module.join("::")),
    }
}

/// Could this path segment be a Rust module name?
fn is_module_name(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Give a unit the owner its file implies.
///
/// Called at the two file-layer boundaries — reading a unit out of the store,
/// and outlining a file live — and nowhere else. Layer 1 stays a pure function
/// of bytes (DEC-021): the `unit` table holds the bare lexical owner, so the
/// same blob still yields the same rows wherever it sits, and this composes the
/// rest on the way out.
///
/// Ruby is untouched. Its files do not declare namespaces; its `class` and
/// `module` keywords do, and the extractor already reads them.
pub fn qualify(path: &str, unit: &mut crate::core::Unit) {
    if unit.lang != crate::core::Lang::Rust {
        return;
    }
    let Some(module) = rust_module(path) else {
        return;
    };
    unit.owner = match unit.owner.is_empty() {
        true => module,
        false => format!("{module}::{}", unit.owner),
    };
}

#[cfg(test)]
mod module_tests {
    use super::*;

    /// The rule, at every shape a real checkout produces. An absolute path and
    /// a repo-relative one must agree, because `--symbols` takes whichever the
    /// caller typed and its answer has to be the id `search` knows.
    #[test]
    fn a_file_names_the_module_it_declares() {
        for (path, expected) in [
            ("src/hash.rs", Some("hash")),
            ("/Users/x/code/contour/src/hash.rs", Some("hash")),
            ("src/summary/contributed.rs", Some("summary::contributed")),
            // A crate root and a `mod.rs` name no module of their own.
            ("src/lib.rs", None),
            ("src/main.rs", None),
            ("src/store/mod.rs", Some("store")),
            ("src/lang/go/mod.rs", Some("lang::go")),
            // An integration test is its own crate root, so `tests/` is the
            // root and not a module named `tests`.
            ("tests/cli_e2e.rs", Some("cli_e2e")),
            ("benches/speed.rs", Some("speed")),
            // A layout with no recognised root keeps the stem. Taking the whole
            // path would put somebody's home directory in a unit's id.
            ("odd/place/thing.rs", Some("thing")),
            ("thing.rs", Some("thing")),
            // A directory that could not be a module name is not one. Without
            // this the answer would depend on how far up the caller stood when
            // they named the file, and fixture directories would land in ids.
            ("tests/testbed/006-rust-names/app.rs", Some("app")),
            ("src/some-crate/lang/go/mod.rs", Some("lang::go")),
        ] {
            assert_eq!(rust_module(path).as_deref(), expected, "{path}");
        }
    }

    /// Ruby files declare no namespace — `class` and `module` do, and the
    /// extractor already reads them. Qualifying one would invent an owner.
    #[test]
    fn only_rust_takes_its_owner_from_its_path() {
        let unit = |lang, owner: &str| crate::core::Unit {
            lang,
            name: "run".into(),
            owner: owner.into(),
            singleton: false,
            params: Vec::new(),
            via: None,
            line: 1,
            end_line: 2,
            norm_hash: None,
            nodes: None,
        };
        let qualified = |lang, owner: &str, path: &str| {
            let mut u = unit(lang, owner);
            qualify(path, &mut u);
            u.id()
        };
        use crate::core::Lang::{Ruby, Rust};
        assert_eq!(qualified(Rust, "", "src/near.rs"), "near::run");
        assert_eq!(
            qualified(Rust, "Widget", "src/near.rs"),
            "near::Widget::run"
        );
        // The crate root adds nothing, so a top-level fn keeps its bare name.
        assert_eq!(qualified(Rust, "", "src/lib.rs"), "run");
        assert_eq!(
            qualified(Ruby, "Widget", "app/models/widget.rb"),
            "Widget#run"
        );
    }

    /// The gap this exists to close: five language plugins, one `tests::find`
    /// each, and nothing able to tell them apart.
    #[test]
    fn identically_named_units_in_different_modules_get_different_ids() {
        let mut ids: Vec<String> = ["src/lang/go/mod.rs", "src/lang/rust/mod.rs"]
            .iter()
            .map(|path| {
                let mut unit = crate::core::Unit {
                    lang: crate::core::Lang::Rust,
                    name: "find".into(),
                    owner: "tests".into(),
                    singleton: false,
                    params: Vec::new(),
                    via: None,
                    line: 1,
                    end_line: 2,
                    norm_hash: None,
                    nodes: None,
                };
                qualify(path, &mut unit);
                unit.id()
            })
            .collect();
        ids.sort();
        assert_eq!(ids, ["lang::go::tests::find", "lang::rust::tests::find"]);
    }

    /// A qualified owner must still classify: 11c ranks a Rust `mod tests`
    /// apart from the code it tests, and it reads the owner to do it.
    #[test]
    fn a_qualified_test_module_is_still_test_code() {
        let mut unit = crate::core::Unit {
            lang: crate::core::Lang::Rust,
            name: "it_works".into(),
            owner: "tests".into(),
            singleton: false,
            params: Vec::new(),
            via: None,
            line: 1,
            end_line: 2,
            norm_hash: None,
            nodes: None,
        };
        qualify("src/summary/contributed.rs", &mut unit);
        assert_eq!(unit.owner, "summary::contributed::tests");
        assert!(in_test_module(&unit));
    }
}

/// Whether a Rust unit sits in a test module, by the name of any enclosing
/// module.
///
/// `tests` and `test` are conventional, and so is a *qualified* name where one
/// file holds several: trekr writes `singleton_tests` and `rails_dsl_tests`
/// beside its plain `tests`, and a rule that matched only the bare name left
/// two thirds of that file's tests ranked as app code.
///
/// A production module genuinely called `test` would be discounted wrongly —
/// survivable precisely because DEC-022 made this a discount rather than an
/// exclusion, and every hit shows the `class` it was given.
fn in_test_module(unit: &crate::core::Unit) -> bool {
    unit.owner.split("::").any(|part| {
        matches!(part, "test" | "tests") || part.ends_with("_test") || part.ends_with("_tests")
    })
}

/// The conventions, in the order a reader would apply them: a spec inside a
/// vendored gem is vendored, and a fixture inside a spec directory is a
/// fixture.
///
/// Deliberately thin. Each rule below is here because a corpus produced a
/// finding that needed it, not because a layout exists somewhere that uses the
/// word — a rule that withholds real code is worse than one that misses.
/// `.contour.toml` is where a repo whose layout differs says so.
fn by_convention(path: &str) -> Class {
    let file = path.rsplit('/').next().unwrap_or(path);
    let dirs: Vec<&str> = path.split('/').rev().skip(1).collect();
    let has = |name: &str| dirs.contains(&name);

    if ["vendor", "node_modules", "third_party", "target", ".bundle"]
        .iter()
        .any(|d| has(d))
    {
        return Class::Vendored;
    }
    // `sorbet/` is tapioca's output tree; `db/schema.rb` is the dumper's.
    if has("sorbet") || (file == "schema.rb" && has("db")) || file.ends_with("_pb.rb") {
        return Class::Generated;
    }
    // A `db` directory, then anything migration-shaped: rails writes
    // `db/migrate`, discourse also writes `db/post_migrate`, and an engine or
    // a plugin nests the whole pair under itself.
    if dirs
        .windows(2)
        .any(|pair| pair[1] == "db" && pair[0].contains("migrat"))
    {
        return Class::Migration;
    }
    if ["fixtures", "fixture", "testbed", "corpus"]
        .iter()
        .any(|d| has(d))
    {
        return Class::Fixture;
    }
    if ["spec", "test", "tests"].iter().any(|d| has(d))
        || file.ends_with("_spec.rb")
        || file.ends_with("_test.rb")
    {
        return Class::Test;
    }
    Class::App
}

/// The file a repository states its own layout in, at the checkout root.
pub const CONFIG_FILE: &str = ".contour.toml";

/// The path policy in force for one command: the conventions above, whatever
/// this checkout's [`CONFIG_FILE`] says, and whatever the run asked for.
///
/// One value, built once by the surface and handed to every query, so `dupes`,
/// `search` and `similar` cannot disagree about what a path is.
#[derive(Debug, Default, Clone)]
pub struct Classes {
    /// Config rules, longest prefix first, consulted ahead of the conventions.
    /// That precedence is the point: it is what lets a repo say `db/migrate`
    /// is ordinary code here.
    rules: Vec<(String, Class)>,
    /// This run asked for everything, so nothing is withheld and nothing is
    /// reported as withheld.
    include_ignored: bool,
}

impl Classes {
    /// The conventions, plus this checkout's config if it has one.
    ///
    /// An absent config is the normal case, not a failure. A config that
    /// exists and cannot be read *is* a failure: quietly ignoring a file
    /// somebody wrote is how a report comes to withhold what they said to
    /// keep.
    pub fn load(root: &Path) -> Result<Classes> {
        let file = root.join(CONFIG_FILE);
        if !file.exists() {
            return Ok(Classes::default());
        }
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        Classes::parse(&text).with_context(|| format!("in {}", file.display()))
    }

    fn parse(text: &str) -> Result<Classes> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Config {
            /// Class name → path prefixes. A typo'd table or class name fails
            /// the run rather than silently classifying nothing, which is the
            /// convention `eval`'s label vocabulary already set.
            #[serde(default)]
            paths: BTreeMap<String, Vec<String>>,
        }

        let config: Config = toml::from_str(text)?;
        let mut rules: Vec<(String, Class)> = Vec::new();
        for (name, prefixes) in config.paths {
            let Some(class) = Class::parse(&name) else {
                let known: Vec<&str> = CLASSES.iter().map(|c| c.as_str()).collect();
                bail!(
                    "`{name}` is not a path class; expected one of {}",
                    known.join(", ")
                );
            };
            for prefix in prefixes {
                let prefix = prefix.trim_end_matches('/').to_string();
                ensure!(!prefix.is_empty(), "a path rule cannot be empty");
                ensure!(
                    !prefix.starts_with('/'),
                    "`{prefix}` must be relative to the checkout root"
                );
                // Prefixes, not globs: a rule means exactly what a SCOPE
                // means, so there is one path language in the tool rather than
                // two that almost agree.
                ensure!(
                    !prefix.contains(['*', '?']),
                    "`{prefix}` is a glob; a path rule is a prefix, matched the way a scope is"
                );
                rules.push((prefix, class));
            }
        }
        // Longest first, so `spec/fixtures` beats `spec` however the file was
        // written — a TOML table has no order a reader could rely on anyway.
        rules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        if let Some(pair) = rules.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            bail!(
                "`{}` is given two classes ({} and {})",
                pair[0].0,
                pair[0].1.as_str(),
                pair[1].1.as_str()
            );
        }
        Ok(Classes {
            rules,
            include_ignored: false,
        })
    }

    /// The one-off override: report every class, and report nothing as
    /// withheld because nothing was.
    pub fn including_ignored(mut self, all: bool) -> Classes {
        self.include_ignored = all;
        self
    }

    pub fn of(&self, path: &str) -> Class {
        self.rules
            .iter()
            .find(|(prefix, _)| under(path, prefix))
            .map(|(_, class)| *class)
            .unwrap_or_else(|| by_convention(path))
    }

    /// The class of one *unit*, which is not always the class of its file.
    ///
    /// Rust puts its unit tests inside the file they test, in a `#[cfg(test)]
    /// mod tests`. The file is app code and the module is not, so a path-only
    /// policy calls `tests::a_body_is_not_its_own_neighbour` app code and
    /// `search` never discounts it — which is exactly the ranking the Rust
    /// field trial tripped on, a page of `tests::*` above the answer.
    ///
    /// Ruby needs no such rule: its tests live in files of their own, which is
    /// why the path policy was enough for two milestones.
    pub fn of_unit(&self, path: &str, unit: &crate::core::Unit) -> Class {
        let class = self.of(path);
        // Only ever *toward* test: an explicit rule saying this tree is app
        // code is a person's decision about their own repo, and a module name
        // should not overrule it.
        match class == Class::App && unit.lang == crate::core::Lang::Rust && in_test_module(unit) {
            true => Class::Test,
            false => class,
        }
    }

    pub fn reports(&self, class: Class) -> bool {
        self.include_ignored || class.reported()
    }

    /// Whether an answer found at `path` is withheld — recording it if so.
    ///
    /// The policy and its disclosure in one call, deliberately: a surface that
    /// could apply one without the other would eventually withhold something
    /// silently, which DEC-022 says is the worse failure.
    pub fn hides(&self, path: &str, withheld: &mut Withheld) -> bool {
        let class = self.of(path);
        if self.reports(class) {
            return false;
        }
        withheld.add(class);
        true
    }
}

/// What the path policy kept out of an answer, by class.
///
/// Disclosed on every run (DEC-022). A report that silently withheld findings
/// would be a worse failure than the one this fixes, so the count travels with
/// the answer in every format rather than being inferable from a short list.
#[derive(Debug, Default, serde::Serialize)]
pub struct Withheld {
    pub total: usize,
    /// Class name → how many. The class *is* the reason, which is why the
    /// breakdown is worth more than the count: "3 withheld" says nothing a
    /// reader can act on, "3 in db/migrate" says whether to look.
    pub by_class: BTreeMap<&'static str, usize>,
}

impl Withheld {
    pub fn add(&mut self, class: Class) {
        self.total += 1;
        *self.by_class.entry(class.as_str()).or_default() += 1;
    }

    pub fn merge(&mut self, other: &Withheld) {
        self.total += other.total;
        for (class, count) in &other.by_class {
            *self.by_class.entry(class).or_default() += count;
        }
    }

    /// The disclosure line, built once so no surface words it differently.
    /// `None` when there is nothing to say.
    ///
    /// Names no flag: this text is shared with the MCP surface, where CLI
    /// syntax is the wrong instruction.
    pub fn note(&self, noun: &str) -> Option<String> {
        if self.total == 0 {
            return None;
        }
        let breakdown: Vec<String> = self
            .by_class
            .iter()
            .map(|(class, count)| format!("{count} {class}"))
            .collect();
        Some(format!(
            "{} {noun}(s) in ignored paths withheld ({})",
            self.total,
            breakdown.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rust keeps its tests in the file they test, so the path cannot answer
    /// this and the unit must. Every module name here is one a real sibling
    /// repo uses: trekr writes `singleton_tests` and `rails_dsl_tests` beside
    /// its plain `tests`, in the very file the field trial's worst ranking
    /// came from.
    #[test]
    fn a_rust_test_module_is_test_code_wherever_its_file_lives() {
        let classes = Classes::default();
        let unit = |owner: &str, lang| crate::core::Unit {
            lang,
            name: "x".into(),
            owner: owner.into(),
            singleton: false,
            params: vec![],
            via: None,
            line: 1,
            end_line: 2,
            norm_hash: None,
            nodes: None,
        };
        let rust =
            |owner: &str| classes.of_unit("src/near.rs", &unit(owner, crate::core::Lang::Rust));
        assert_eq!(rust("tests"), Class::Test);
        assert_eq!(rust("test"), Class::Test);
        assert_eq!(rust("singleton_tests"), Class::Test);
        assert_eq!(rust("rails_dsl_tests"), Class::Test);
        assert_eq!(rust("near::tests"), Class::Test, "nested in a module");
        assert_eq!(rust(""), Class::App, "a free function is not a test");
        assert_eq!(rust("Contour"), Class::App);
        // `latest` ends in `test`, and is not one. The suffix rule is about a
        // word boundary, which is what the underscore is doing.
        assert_eq!(rust("latest"), Class::App);

        // Ruby's tests live in files of their own, and a Ruby class called
        // `Test` must not be discounted for its name.
        assert_eq!(
            classes.of_unit("app/models/user.rb", &unit("Test", crate::core::Lang::Ruby)),
            Class::App
        );
        // The file still decides when it already says test.
        assert_eq!(
            classes.of_unit("tests/cli_e2e.rs", &unit("", crate::core::Lang::Rust)),
            Class::Test
        );
    }

    /// Each row is a finding from a real corpus, not a layout someone imagined:
    /// discourse's plugin migrations, trekr's fixture corpus, rwr's `corpus/`,
    /// berater's spec helper, trekr's tapioca output.
    #[test]
    fn a_path_says_what_kind_of_file_it_is() {
        let classes = Classes::default();
        let cases = [
            ("app/models/user.rb", Class::App),
            ("lib/contour/search.rb", Class::App),
            ("src/search.rs", Class::App),
            ("spec/lib/limiter_spec.rb", Class::Test),
            ("test/cases/base_test.rb", Class::Test),
            ("tests/cli_e2e.rs", Class::Test),
            // A spec file outside any spec directory still names itself.
            ("app/models/user_spec.rb", Class::Test),
            ("tests/fixtures/widget.rb", Class::Fixture),
            ("corpus/001-return-nil/in/basic.rb", Class::Fixture),
            ("tests/testbed/001-macros/app.rb", Class::Fixture),
            ("db/migrate/20240101_add_users.rb", Class::Migration),
            ("db/post_migrate/20240101_backfill.rb", Class::Migration),
            ("plugins/chat/db/migrate/20240101_x.rb", Class::Migration),
            ("db/schema.rb", Class::Generated),
            ("sorbet/rbi/gems/widget@1.0.0.rbi", Class::Generated),
            ("vendor/bundle/gems/rack/lib/rack.rb", Class::Vendored),
            ("node_modules/x/y.rb", Class::Vendored),
            // A vendored gem's own specs are vendored, not tests: the order of
            // the conventions is a claim, so it gets an assertion.
            ("vendor/gems/rack/spec/rack_spec.rb", Class::Vendored),
        ];
        for (path, expected) in cases {
            assert_eq!(classes.of(path), expected, "{path}");
        }

        // The negatives that keep the conventions from eating real code: a
        // directory is not a segment because it shares a prefix with one, and
        // a *file* named for a class is still app code.
        for path in [
            "app/models/testimonial.rb",
            "app/services/vendors/invoice.rb",
            "lib/specifications.rb",
            "app/models/vendor.rb",
            "db/models/user.rb",
        ] {
            assert_eq!(classes.of(path), Class::App, "{path}");
        }
    }

    /// The defaults the owner ruled (DEC-022). Tests are the one most likely
    /// to be "fixed" into an exclusion, so it is pinned here.
    #[test]
    fn only_frozen_and_regenerated_code_is_ignored_by_default() {
        let reported: Vec<&str> = CLASSES
            .iter()
            .filter(|c| c.reported())
            .map(|c| c.as_str())
            .collect();
        assert_eq!(reported, ["app", "test", "fixture"]);

        // And the one-off override reports everything, without pretending
        // anything was withheld.
        let all = Classes::default().including_ignored(true);
        assert!(CLASSES.iter().all(|c| all.reports(*c)));
    }

    #[test]
    fn a_repo_can_say_its_layout_differs() {
        let classes = Classes::parse(
            r#"
            [paths]
            app = ["db/migrate"]
            test = ["examples"]
            fixture = ["examples/data"]
            "#,
        )
        .unwrap();
        // The override beats the convention, which is the whole point of it.
        assert_eq!(classes.of("db/migrate/20240101_x.rb"), Class::App);
        assert!(!classes.hides("db/migrate/20240101_x.rb", &mut Withheld::default()));
        assert_eq!(classes.of("examples/tour.rb"), Class::Test);
        // Longest prefix wins, whatever order the table happened to be in.
        assert_eq!(classes.of("examples/data/tour.rb"), Class::Fixture);
        // Anything the config does not name still falls through to convention.
        assert_eq!(classes.of("spec/tour_spec.rb"), Class::Test);
    }

    /// A config that says something wrong fails the run. Silently classifying
    /// nothing is how a report comes to withhold what somebody said to keep.
    #[test]
    fn a_config_that_says_something_wrong_is_refused() {
        for bad in [
            r#"[paths]
               tests = ["spec"]"#,
            r#"[path]
               test = ["spec"]"#,
            r#"[paths]
               test = ["spec/**/*.rb"]"#,
            r#"[paths]
               test = ["/abs/spec"]"#,
            r#"[paths]
               test = [""]"#,
            r#"[paths]
               test = ["shared"]
               fixture = ["shared"]"#,
        ] {
            assert!(Classes::parse(bad).is_err(), "{bad}");
        }
        // An empty file is a repo that says nothing, not a broken one.
        assert!(Classes::parse("").is_ok());
    }

    #[test]
    fn a_disclosure_names_the_reason_not_just_the_count() {
        let mut withheld = Withheld::default();
        assert_eq!(withheld.note("group"), None, "nothing to say");
        withheld.add(Class::Migration);
        withheld.add(Class::Migration);
        withheld.add(Class::Vendored);
        assert_eq!(
            withheld.note("group").unwrap(),
            "3 group(s) in ignored paths withheld (2 migration, 1 vendored)"
        );

        let mut other = Withheld::default();
        other.add(Class::Migration);
        withheld.merge(&other);
        assert_eq!(withheld.total, 4);
        assert_eq!(withheld.by_class["migration"], 3);
    }

    #[test]
    fn a_scope_stops_at_a_path_boundary() {
        assert!(under("app/models/widget.rb", "app/models"));
        assert!(under("app/models/widget.rb", "app/models/"));
        assert!(under("app/models/widget.rb", "app/models/widget.rb"));
        assert!(!under("app/models2/widget.rb", "app/models"));
        assert!(under("anything", "."));
    }

    #[test]
    fn a_path_round_trips_between_the_two_audiences() {
        let abs = absolute("/repo", "app/models/x.rb");
        assert_eq!(abs, "/repo/app/models/x.rb");
        assert_eq!(within("/repo", &abs), "app/models/x.rb");
        // A trailing slash on the root is a real shape and must not double up.
        assert_eq!(absolute("/repo/", "a.rb"), "/repo/a.rb");
        assert_eq!(within("/repo/", "/repo/a.rb"), "a.rb");
        // A path from somewhere else is left alone rather than mangled.
        assert_eq!(within("/repo", "/elsewhere/a.rb"), "/elsewhere/a.rb");
    }

    #[test]
    fn shortens_home_and_nothing_else() {
        // SAFETY: single-threaded test, restored before it returns.
        let before = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", "/Users/dan") };

        assert_eq!(pretty("/Users/dan/code/app.rb"), "~/code/app.rb");
        assert_eq!(pretty("/Users/dan"), "~");
        assert_eq!(pretty("/Users/danger/x.rb"), "/Users/danger/x.rb");
        // A trailing slash on HOME is a real shape and must not double up.
        unsafe { std::env::set_var("HOME", "/Users/dan/") };
        assert_eq!(pretty("/Users/dan/code/app.rb"), "~/code/app.rb");

        match before {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
