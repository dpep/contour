//! Which duplicate groups offer a consolidation that does not exist.
//!
//! rails' `Compatibility::V7_0#compatible_table_definition` and its `V6_1`
//! sibling are byte-identical and group as exact clones. Each body's
//! unqualified `prepend TableDefinition` resolves to its *own* version
//! module's nested class, so the copies do different things and merging them
//! is not a consolidation. The labeling rule this report answers to is that a
//! pair is a duplicate iff consolidating it would reduce net complexity — and
//! here it would not.
//!
//! DEC-017 folded `super`'s enclosing name into `norm_hash`, because `super`
//! is **always** name-dependent. An unqualified constant is only *sometimes*
//! context-dependent, and the difference was measured on rails' 296 exact
//! groups:
//!
//! | policy | groups affected |
//! | ------ | --------------- |
//! | fold the nesting at every unqualified constant read | splits 127 (43%) |
//! | caveat every group whose bodies read one | flags 165 (56%) |
//! | caveat only where such a constant is defined under >1 nesting | flags 54 (18%) |
//!
//! So this is a **caveat on the report, never a fold into the hash**: no
//! reindex, no resummarize, DEC-003's key stays a pure function of the body.
//! The third row is the ratified policy, and the narrowing is what `rq` buys —
//! which is why an absent `rq` reports the check as unavailable rather than
//! falling back to the noisy row nobody reads.
//!
//! **As built it narrows further, to 8% (22 of 292).** The ratified row asks
//! whether a constant is defined under more than one nesting *anywhere*; this
//! asks whether the copies **reach different definitions of it**, which is
//! Ruby's own lexical rule and one filter more (see [`resolve`]). The
//! difference is not cosmetic: rails defines `Array` inside a PostgreSQL
//! adapter as well as at the top level, so the looser rule flags every pair of
//! copies that mentions `Array` — 44 groups, half of them noise — and a
//! warning that is usually wrong is a warning nobody reads. What survives is
//! specific: `TableDefinition` across three version modules, `READ_QUERY`
//! across three adapters, `DATE_FORMATS` across `Date` and `Time`.

use crate::dupes::Group;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// Why a group's consolidation may not be available.
#[derive(Debug, serde::Serialize)]
pub struct Caveat {
    /// The constants read under more than one nesting here, each of which is
    /// also defined under more than one nesting somewhere in the checkout.
    /// Named rather than counted: the reader's next move is to go and look at
    /// one, and a bare "may not consolidate" gives them nowhere to start.
    pub constants: Vec<String>,
    /// One line saying what to check, in the report's own voice.
    pub basis: String,
}

/// What the pass spent and what it could not answer.
#[derive(Debug, Default, serde::Serialize)]
pub struct Stats {
    /// Groups whose members sit under different nestings and read the same
    /// unqualified constant — the population `rq` was asked to narrow.
    pub candidates: usize,
    pub rq_probes: usize,
    pub millis: u128,
    /// Set when the narrowing could not run. The candidates are then reported
    /// as unchecked rather than flagged, because flagging 56% of the report is
    /// the noise this policy exists to avoid — and silently flagging none
    /// would crown consolidations that may not exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// Attach a caveat to every group whose copies may not be interchangeable.
///
/// Two passes, cheap before expensive: read the bodies to find groups that
/// even *could* be affected, then ask `rq` about only the constants those
/// groups named.
pub fn annotate(root: &Path, groups: &mut [Group]) -> Stats {
    let started = std::time::Instant::now();
    let mut stats = Stats::default();

    // Per candidate group: each unqualified constant its copies read, and the
    // nesting each copy reads it under.
    let mut candidates: Vec<(usize, HashMap<String, Vec<String>>)> = Vec::new();
    let mut sources: HashMap<String, Vec<crate::core::ConstRead>> = HashMap::new();
    for (index, group) in groups.iter().enumerate() {
        if group.lang != crate::core::Lang::Ruby {
            continue;
        }
        // Same nesting everywhere means every copy resolves every constant the
        // same way, whatever those constants are — nothing to caveat.
        let owners: HashSet<&str> = group.members.iter().map(|m| m.owner.as_str()).collect();
        if owners.len() < 2 {
            continue;
        }
        let read = read_under_two_nestings(group, &mut sources);
        if read.is_empty() {
            continue;
        }
        stats.candidates += 1;
        candidates.push((index, read));
    }

    let mut names: Vec<String> = candidates
        .iter()
        .flat_map(|(_, read)| read.keys().cloned())
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        stats.millis = started.elapsed().as_millis();
        return stats;
    }
    let defined: HashMap<&String, Definitions> = names
        .par_iter()
        .map(|name| (name, definitions(root, name)))
        .collect();
    stats.rq_probes = defined.len();

    for (index, read) in &candidates {
        for (name, nestings) in read {
            match &defined[name] {
                Definitions::Unavailable(why) => {
                    stats.unavailable.get_or_insert(why.clone());
                }
                Definitions::Nestings(defined) => {
                    // The narrowing, and the whole value of the probe: two
                    // copies are only at risk when the definition each one
                    // actually reaches is a different definition.
                    let reached: HashSet<Option<&String>> =
                        nestings.iter().map(|at| resolve(defined, at)).collect();
                    if reached.len() > 1 {
                        push(&mut groups[*index], name);
                    }
                }
            }
        }
    }
    stats.millis = started.elapsed().as_millis();
    stats
}

/// Which definition of `name` a body written at `nesting` actually reaches.
///
/// Ruby looks outward through the lexical scope — for a read inside
/// `A::B::C`, it tries `A::B::C`, then `A::B`, then `A`, then the top level —
/// so the innermost enclosing definition wins. That is what makes this a
/// *caveat* rather than noise: rails defines `Array` under
/// `ActiveRecord::ConnectionAdapters::PostgreSQL::OID`, but two copies in
/// `ActiveSupport` both reach the top-level one and are interchangeable.
///
/// Lexical only, and deliberately: definitions reached through an ancestor
/// chain are a tree-layer question contour does not answer (DEC-013), and a
/// caveat that guessed at them would be a guess wearing a warning's clothes.
fn resolve<'a>(defined: &'a HashSet<String>, nesting: &str) -> Option<&'a String> {
    defined
        .iter()
        .filter(|at| {
            at.is_empty() || nesting == at.as_str() || nesting.starts_with(&format!("{at}::"))
        })
        .max_by_key(|at| at.len())
}

fn push(group: &mut Group, name: &str) {
    let caveat = group.caveat.get_or_insert_with(|| Caveat {
        constants: Vec::new(),
        basis: String::new(),
    });
    caveat.constants.push(name.to_string());
    caveat.constants.sort();
    caveat.basis = format!(
        "copies sit under different namespaces and read {}, which is defined under more than one — check that they resolve to the same thing before consolidating",
        caveat.constants.join(", ")
    );
}

/// The unqualified constants this group's copies read under differing
/// nestings.
///
/// Exact clones have identical bodies, so every copy reads the same names; the
/// nesting each is read under is what differs, and that is the condition.
fn read_under_two_nestings(
    group: &Group,
    sources: &mut HashMap<String, Vec<crate::core::ConstRead>>,
) -> HashMap<String, Vec<String>> {
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    for member in &group.members {
        let reads = sources.entry(member.path.clone()).or_insert_with(|| {
            // A file that cannot be read contributes nothing rather than
            // failing the report: the group is still a real finding, and the
            // caveat is an annotation on it.
            std::fs::read(&member.path)
                .map(|src| crate::ruby::const_reads(&src))
                .unwrap_or_default()
        });
        for read in reads.iter() {
            if read.line >= member.line && read.line <= member.end_line && read.is_unqualified() {
                seen.entry(read.name.clone())
                    .or_default()
                    .insert(read.nesting.clone());
            }
        }
    }
    seen.into_iter()
        .filter(|(_, nestings)| nestings.len() > 1)
        .map(|(name, nestings)| (name, nestings.into_iter().collect()))
        .collect()
}

enum Definitions {
    /// Every distinct nesting that defines this constant, `::`-joined, empty
    /// for a top-level definition. The *set* rather than a count, because
    /// which one a body reaches is the question (see [`resolve`]).
    Nestings(HashSet<String>),
    Unavailable(String),
}

/// Ask rq how many distinct namespaces define one constant.
///
/// `$CONTOUR_RQ` names the binary, following `canonical`'s trekr probe exactly
/// — including why: a checkout can be reported on without it, and the tests
/// stay hermetic by pointing it at something unrunnable.
fn definitions(root: &Path, name: &str) -> Definitions {
    let binary = std::env::var("CONTOUR_RQ").unwrap_or_else(|_| "rq".to_string());
    let out = match Command::new(&binary)
        // `-l 0` because the question is how many, not which is best; and
        // `--no-record` so a report does not teach rq's ranking that a human
        // searched for this.
        .args([
            name,
            "-j",
            "-l",
            "0",
            "-x",
            "ruby",
            "--no-record",
            "--no-wait",
        ])
        .current_dir(root)
        .output()
    {
        Ok(out) => out,
        Err(_) => return Definitions::Unavailable(format!("`{binary}` is not installed")),
    };
    count_nestings(&out.stdout, name, &binary)
}

/// The parsing, apart from the process, because it is the half that can be
/// wrong in silence.
///
/// It already was: reading rq's array as `{"results": [...]}` returned "zero
/// definitions" for every constant in rails, which reads exactly like "nothing
/// to caveat" and flagged nothing at all. A miscount here does not raise an
/// error — it quietly withdraws the warning this module exists to give — so
/// the shapes are pinned by tests rather than by a run that looked plausible.
fn count_nestings(stdout: &[u8], name: &str, binary: &str) -> Definitions {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(stdout) else {
        return Definitions::Unavailable(format!("`{binary}` returned no answer"));
    };
    let results = match (value.as_array(), value["status"].as_str()) {
        // A hit is a bare array of definitions.
        (Some(results), _) => results,
        // A miss is an object saying so, and is a real count of zero.
        (None, Some("no_match")) => return Definitions::Nestings(HashSet::new()),
        // Anything else — a warming index above all — is a question rq did
        // not answer, and reading it as zero would drop the caveat silently.
        (None, Some(status)) => return Definitions::Unavailable(format!("rq: {status}")),
        (None, None) => return Definitions::Unavailable(format!("`{binary}` returned no answer")),
    };
    // rq ranks fuzzily, so the exact-name filter is ours to apply.
    // A top-level definition has no `parent` key at all, which is the empty
    // nesting rather than a missing answer.
    let nestings: HashSet<String> = results
        .iter()
        .filter(|r| r["name"].as_str() == Some(name))
        .map(|r| r["parent"].as_str().unwrap_or("").to_string())
        .collect();
    Definitions::Nestings(nestings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(json: &str) -> Definitions {
        count_nestings(json.as_bytes(), "TableDefinition", "rq")
    }

    /// rq's real shapes, abridged from actual output against rails.
    #[test]
    fn rq_answers_are_read_as_rq_writes_them() {
        let two = r#"[
          {"name":"TableDefinition","parent":"ActiveRecord::ConnectionAdapters"},
          {"name":"TableDefinition","parent":"ActiveRecord::ConnectionAdapters::PostgreSQL"}
        ]"#;
        assert!(matches!(count(two), Definitions::Nestings(ref n) if n.len() == 2));

        // One definition is the common case and the whole point of the probe:
        // a constant defined once resolves the same way from anywhere.
        let one = r#"[{"name":"TableDefinition","parent":"ActiveRecord"}]"#;
        assert!(matches!(count(one), Definitions::Nestings(ref n) if n.len() == 1));

        // Ranked fuzzily, so a near-name in the results is not a definition
        // of the constant that was asked about.
        let fuzzy = r#"[
          {"name":"TableDefinition","parent":"A"},
          {"name":"TableDefinitions","parent":"B"}
        ]"#;
        assert!(matches!(count(fuzzy), Definitions::Nestings(ref n) if n.len() == 1));

        assert!(matches!(
            count(r#"{"query":"Nope","status":"no_match"}"#),
            Definitions::Nestings(ref n) if n.is_empty()
        ));
    }

    /// The rule that separates a caveat from noise, in the two rails cases
    /// that motivated it.
    #[test]
    fn only_the_definition_a_body_can_see_counts() {
        // `TableDefinition` is defined inside each version module. Two copies
        // written in different version modules reach different definitions —
        // the case the whole caveat exists for.
        let versioned: HashSet<String> = ["ActiveRecord::ConnectionAdapters", "V7_0", "V6_1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_ne!(
            resolve(&versioned, "V7_0"),
            resolve(&versioned, "V6_1"),
            "each version module reaches its own"
        );

        // `Array` is defined at the top level and once, deep inside a
        // PostgreSQL adapter. Two copies in ActiveSupport both reach the
        // top-level one, so they are interchangeable and must not be flagged.
        let core: HashSet<String> = ["", "ActiveRecord::ConnectionAdapters::PostgreSQL::OID"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            resolve(&core, "ActiveSupport::XmlMini_JDOM"),
            resolve(&core, "ActiveSupport::XmlMini_REXML"),
            "neither can see the adapter's"
        );

        // A prefix that is not a namespace boundary is not an enclosing scope:
        // `Foo` does not enclose `Foobar`.
        let near: HashSet<String> = ["Foo".to_string()].into_iter().collect();
        assert_eq!(resolve(&near, "Foobar"), None);
        assert_eq!(resolve(&near, "Foo::Inner"), Some(&"Foo".to_string()));
    }

    /// The direction that must never be silent: not knowing is not zero.
    #[test]
    fn an_unanswered_probe_is_unavailable_not_zero() {
        assert!(matches!(
            count(r#"{"status":"indexing","hint":"rq --wait"}"#),
            Definitions::Unavailable(_)
        ));
        assert!(matches!(
            count("not json at all"),
            Definitions::Unavailable(_)
        ));
    }
}
