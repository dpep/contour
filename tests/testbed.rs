//! One harness, many cases. Adapted from trekr's `tests/testbed.rs`.
//!
//! Every case in `tests/testbed/` is a directory holding a tiny Ruby source
//! tree and an `expected` file, so **adding a case is dropping in files — no
//! Rust**. Each case is staged as a real git checkout with its own database
//! and indexed, which exercises the whole path: extract → store → CLI.
//!
//! See `tests/testbed/README.md` for the `expected` format. An unknown verb or
//! key fails loudly: a typo in an expectation is a test that proves nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    git_at(dir, "2000-01-01T00:00:00Z", args);
}

/// The same, with the commit dated. Every commit a case makes is dated
/// explicitly: `git blame` is a canonicality signal, so "when was this
/// written" has to be a property of the fixture rather than of the machine
/// that ran the suite.
fn git_at(dir: &Path, date: &str, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// Stage one case as a real checkout with its own database.
fn stage(case: &Path, label: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir();
    let dir = base.join(format!("contour-bed-{}-{label}", std::process::id()));
    let db = base.join(format!("contour-bed-{}-{label}.db", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", db.display()));
    }
    copy_tree(case, &dir);
    let _ = fs::remove_file(dir.join("expected"));
    let _ = fs::remove_file(dir.join("README.md"));

    // A case may hand its files a history: each path in `history` gets its own
    // commit, in order, a year apart, and everything else lands in the first.
    // Without it the whole case is one commit — which is itself a fixture
    // worth having, since it is what makes the git-age signal tie.
    let history: Vec<String> = fs::read_to_string(case.join("history"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let _ = fs::remove_file(dir.join("history"));

    git(&dir, &["init", "-q"]);
    let author = ["-c", "user.email=t@e.st", "-c", "user.name=test"];
    // Everything the history does not name goes into the first commit, so a
    // case only has to date the files it is making a claim about.
    let mut rest: Vec<String> = vec!["-A".into(), "--".into(), ".".into()];
    rest.extend(history.iter().map(|path| format!(":!:{path}")));
    for (nth, path) in std::iter::once(None)
        .chain(history.iter().map(Some))
        .enumerate()
    {
        match path {
            None => {
                let args: Vec<&str> = std::iter::once("add")
                    .chain(rest.iter().map(String::as_str))
                    .collect();
                git(&dir, &args);
            }
            Some(path) => git(&dir, &["add", "-A", "--", path]),
        }
        let year = format!("{}-01-01T00:00:00Z", 2000 + nth);
        let staged = Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(&dir)
            .output()
            .expect("run git");
        // An empty commit would still be a commit, and dating one is how a
        // case accidentally makes every body look the same age.
        if !staged.status.success() {
            git_at(
                &dir,
                &year,
                &[&author[..], &["commit", "-qm", "case"]].concat(),
            );
        }
    }

    let indexed = contour(&db, &dir, &["index"]);
    assert!(
        indexed.1 == 0,
        "indexing {label} failed: {}",
        indexed.2.trim()
    );
    (dir, db)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Run the built binary against the staged checkout. Returns parsed stdout,
/// the exit code, and stderr.
fn contour(db: &Path, dir: &Path, args: &[&str]) -> (serde_json::Value, i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_contour"))
        .args(args)
        .current_dir(dir)
        .env("CONTOUR_DB", db)
        // Hermetic: a case must not consult whatever trekr this machine has,
        // nor index a temp repo into its global store. The reference signal
        // reports itself absent, which is the degraded path a case can pin.
        .env("CONTOUR_TREKR", "/nonexistent/trekr")
        .output()
        .expect("run contour");
    (
        serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null),
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The trailing field of an assertion line: everything after the verb and its
/// target, trimmed.
fn tail(rest: &str) -> &str {
    // Trimmed first: `split_once` consumes exactly one space, so a case that
    // lines its columns up with two would otherwise get the whole rest back.
    rest.trim()
        .split_once(char::is_whitespace)
        .map(|(_, t)| t.trim())
        .unwrap_or_default()
}

#[test]
fn every_testbed_case_answers_as_recorded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testbed");
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .expect("tests/testbed exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no cases in {}", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut checks = 0usize;
    for case in &cases {
        let label = case.file_name().unwrap().to_string_lossy().into_owned();
        let expectations = fs::read_to_string(case.join("expected"))
            .unwrap_or_else(|_| panic!("{label} has no `expected` file"));
        let (dir, db) = stage(case, &label);

        for line in expectations.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            checks += 1;
            let mut fail = |what: String| failures.push(format!("{label}: {line}\n      {what}"));
            let Some((verb, rest)) = line.split_once(char::is_whitespace) else {
                fail("cannot parse".into());
                continue;
            };
            let target = rest.split_whitespace().next().unwrap_or_default();
            match verb {
                // `symbols FILE  Widget#save,Widget.find` — the outline, in
                // source order.
                "symbols" => {
                    let (answer, _, _) = contour(&db, &dir, &["--symbols", target, "--json"]);
                    let got: Vec<String> = answer["units"]
                        .as_array()
                        .map(|rows| rows.iter().filter_map(unit_id).collect())
                        .unwrap_or_default();
                    let want: Vec<&str> = tail(rest).split(',').filter(|s| !s.is_empty()).collect();
                    if got != want {
                        fail(format!("expected {want:?}, got {got:?}"));
                    }
                }
                // `dupes  A#x+B#y  C#z+D#w` — the WHOLE report, so a case
                // pins what does not collide as firmly as what does. That
                // totality is the point: "a rename collides" is only half a
                // claim without "changed logic does not".
                "dupes" => {
                    // Permissive parameters on purpose, for the same reason
                    // `--min-lines 1` is here: fixtures are small, and
                    // Jaccard is harsher on a small body (one edit moves
                    // every ancestor subtree, which is a larger share of a
                    // short signature). The testbed pins *semantics*; the
                    // calibrated defaults are pinned by the rails eval,
                    // against a corpus where they mean something.
                    let (answer, _, _) = contour(
                        &db,
                        &dir,
                        &[
                            "dupes",
                            "--min-lines",
                            "1",
                            "--near",
                            "--near-threshold",
                            "0.5",
                            "--json",
                        ],
                    );
                    let got = clone_groups(&answer["groups"]);
                    let mut want: Vec<String> = rest
                        .split_whitespace()
                        .filter(|g| *g != "(none)")
                        .map(|g| {
                            let mut ids: Vec<&str> = g.split('+').collect();
                            ids.sort_unstable();
                            ids.join("+")
                        })
                        .collect();
                    want.sort();
                    if got != want {
                        fail(format!("expected {want:?}, got {got:?}"));
                    }
                }
                // `canonical  A#x+B#y  A#x` — which member of that group the
                // signals name, or `(none)` when they decline to. An
                // abstention is an expectation like any other: it is what a
                // group with nothing to go on is supposed to produce.
                "canonical" => {
                    let (answer, _, _) = contour(
                        &db,
                        &dir,
                        &[
                            "dupes",
                            "--min-lines",
                            "1",
                            "--near",
                            "--near-threshold",
                            "0.5",
                            "--canonical",
                            "--json",
                        ],
                    );
                    let mut want_ids: Vec<&str> = target.split('+').collect();
                    want_ids.sort_unstable();
                    let found = answer["groups"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|group| {
                            let mut ids: Vec<&str> = group["members"]
                                .as_array()
                                .map(|ms| ms.iter().filter_map(|m| m["id"].as_str()).collect())
                                .unwrap_or_default();
                            ids.sort_unstable();
                            ids == want_ids
                        });
                    match found {
                        None => fail(format!("no group holds exactly {want_ids:?}")),
                        Some(group) => {
                            let got = group["canonical"]["pick"]["id"]
                                .as_str()
                                .unwrap_or("(none)");
                            let want = tail(rest);
                            if got != want {
                                fail(format!(
                                    "expected {want:?}, got {got:?} — {}",
                                    group["canonical"]["basis"].as_str().unwrap_or("?")
                                ));
                            }
                        }
                    }
                }
                other => fail(format!("unknown verb `{other}`")),
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    assert!(
        failures.is_empty(),
        "{} of {checks} testbed checks failed across {} cases:\n\n  {}\n",
        failures.len(),
        cases.len(),
        failures.join("\n\n  ")
    );
}

/// The clone report as comparable text: each group's ids sorted and joined by
/// `+`, then the groups sorted.
///
/// Sorted, so a case pins *which* units collide and not how the report ranks
/// them. Ranking is a presentation choice that should be free to change; what
/// hashes together is not.
fn clone_groups(answer: &serde_json::Value) -> Vec<String> {
    let mut groups: Vec<String> = answer
        .as_array()
        .map(|gs| {
            gs.iter()
                .map(|g| {
                    let mut ids: Vec<&str> = g["members"]
                        .as_array()
                        .map(|ms| ms.iter().filter_map(|m| m["id"].as_str()).collect())
                        .unwrap_or_default();
                    ids.sort_unstable();
                    ids.join("+")
                })
                .collect()
        })
        .unwrap_or_default();
    groups.sort();
    groups
}

/// Rebuild `Unit::id` from JSON, so an expectation names a unit the way a
/// person does rather than the way the record is shaped.
///
/// Mirrors `core::Unit::id`, including its one language-specific bit: Rust
/// spells every path with `::`, Ruby distinguishes `#` from `.`.
fn unit_id(row: &serde_json::Value) -> Option<String> {
    let name = row["name"].as_str()?;
    let owner = row["owner"].as_str().unwrap_or_default();
    if owner.is_empty() {
        return Some(name.to_string());
    }
    let sep = match (row["lang"].as_str(), row["singleton"].as_bool()) {
        (Some("rust"), _) => "::",
        (_, Some(true)) => ".",
        _ => "#",
    };
    Some(format!("{owner}{sep}{name}"))
}
