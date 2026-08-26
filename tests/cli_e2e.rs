//! The built binary, driven against a real checkout with an isolated database.
//!
//! What the testbed cannot pin: exit codes, the `--json`/`--ndjson` split, and
//! the index/status round trip.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Repo {
    dir: PathBuf,
    db: PathBuf,
}

impl Repo {
    fn new(label: &str, files: &[(&str, &str)]) -> Repo {
        let base = std::env::temp_dir();
        let dir = base.join(format!("contour-e2e-{}-{label}", std::process::id()));
        let db = base.join(format!("contour-e2e-{}-{label}.db", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
        }
        std::fs::create_dir_all(&dir).unwrap();
        for (path, body) in files {
            let target = dir.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, body).unwrap();
        }
        git(&dir, &["init", "-q"]);
        git(&dir, &["add", "-A"]);
        git(
            &dir,
            &[
                "-c",
                "user.email=t@e.st",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "case",
            ],
        );
        Repo { dir, db }
    }

    fn run(&self, args: &[&str]) -> (String, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_contour"))
            .args(args)
            .current_dir(&self.dir)
            .env("CONTOUR_DB", &self.db)
            .output()
            .expect("run contour");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            out.status.code().unwrap_or(-1),
        )
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let (out, _) = self.run(args);
        serde_json::from_str(&out).unwrap_or(serde_json::Value::Null)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

#[test]
fn indexing_a_checkout_then_asking_about_it() {
    let repo = Repo::new(
        "index",
        &[
            ("app/widget.rb", "class Widget\n  def save; end\nend\n"),
            (
                "lib/job.rb",
                "class Job\n  def run; end\n  def self.all; end\nend\n",
            ),
            ("README.md", "not ruby\n"),
        ],
    );

    let indexed = repo.json(&["index", "--json"]);
    assert_eq!(indexed["indexed"]["files"], 2, "README.md is not Ruby");
    assert_eq!(indexed["indexed"]["parsed"], 2);
    assert_eq!(indexed["indexed"]["units"], 3);

    // A second pass parses nothing: same bytes, same blobs, same units.
    let again = repo.json(&["index", "--json"]);
    assert_eq!(again["indexed"]["parsed"], 0);
    assert_eq!(
        again["indexed"]["units"], 0,
        "units counts work, not contents"
    );

    let status = repo.json(&["--status", "--json"]);
    assert_eq!(status["checkouts"][0]["units"], 3);
    assert_eq!(status["checkouts"][0]["files"], 2);
    // Coverage is per model, and nothing has been summarized, so no model has
    // an opinion yet.
    assert_eq!(
        status["checkouts"][0]["coverage"].as_array().map(Vec::len),
        Some(0)
    );
}

/// Exit codes are the scriptable half of every answer: 0 found, 1 nothing to
/// report, 2 could not answer.
#[test]
fn exit_codes_say_hit_miss_and_error() {
    let repo = Repo::new(
        "codes",
        &[
            ("a.rb", "def helper; end\n"),
            ("empty.rb", "# nothing here\n"),
        ],
    );
    assert_eq!(repo.run(&["--symbols", "a.rb"]).1, 0);
    assert_eq!(repo.run(&["--symbols", "empty.rb"]).1, 1);
    assert_eq!(repo.run(&["--symbols", "missing.rb"]).1, 2);
    assert_eq!(repo.run(&["--status"]).1, 1, "nothing indexed yet");
    assert_eq!(repo.run(&["index"]).1, 0);
    assert_eq!(repo.run(&["--status"]).1, 0);
    assert_eq!(
        repo.run(&["--symbols", "a.rb", "--json", "--ndjson"]).1,
        2,
        "two answers to one question"
    );
}

/// `--symbols` parses the file in front of it. That it works with no index is
/// the rule the CLI is organised around: flags do not touch the index.
#[test]
fn symbols_needs_no_index() {
    let repo = Repo::new(
        "live",
        &[("a.rb", "class Widget\n  def save(force:); end\nend\n")],
    );
    let rows = repo.json(&["--symbols", "a.rb", "--json"]);
    assert_eq!(rows[0]["name"], "save");
    assert_eq!(rows[0]["owner"], "Widget");
    assert_eq!(rows[0]["params"][0]["kind"], "keyreq");

    // `--ndjson` is one compact object per unit, not one pretty document.
    let (ndjson, _) = repo.run(&["--symbols", "a.rb", "--ndjson"]);
    assert_eq!(ndjson.lines().count(), 1);
    assert!(serde_json::from_str::<serde_json::Value>(ndjson.trim()).is_ok());
}

/// An edited file is a new blob, so its units are re-read while its
/// neighbours' are not.
#[test]
fn an_edit_reparses_only_what_moved() {
    let repo = Repo::new(
        "edit",
        &[
            ("a.rb", "class A\n  def one; end\nend\n"),
            ("b.rb", "class B\n  def two; end\nend\n"),
        ],
    );
    repo.run(&["index"]);
    std::fs::write(
        repo.dir.join("a.rb"),
        "class A\n  def one; end\n  def three; end\nend\n",
    )
    .unwrap();

    let indexed = repo.json(&["index", "--json"]);
    assert_eq!(indexed["indexed"]["parsed"], 1, "only a.rb is a new blob");
    assert_eq!(
        repo.json(&["--status", "--json"])["checkouts"][0]["units"],
        3
    );
}

/// The clone report's floor, and the scope filter. The testbed pins what
/// hashes together; this pins what the command chooses to show.
#[test]
fn dupes_hides_bodies_too_short_to_mean_anything() {
    let short = "class %C%\n  def get\n    @thing\n  end\nend\n";
    let long = "class %C%\n  def run(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "dupes",
        &[
            ("app/a.rb", &short.replace("%C%", "Alpha")),
            ("app/b.rb", &short.replace("%C%", "Beta")),
            ("app/c.rb", &long.replace("%C%", "Gamma")),
            ("lib/d.rb", &long.replace("%C%", "Delta")),
        ],
    );
    repo.run(&["index"]);

    // The two three-line accessors are identical because there is only one way
    // to write them; the default floor drops them.
    let groups = repo.json(&["dupes", "--json"]);
    assert_eq!(groups.as_array().map(Vec::len), Some(1));
    assert_eq!(groups[0]["lines"], 5);
    assert_eq!(groups[0]["how"], "structural");
    // A u64 past 2^53 does not survive a JSON parser that stores numbers as
    // doubles, so the key travels as hex.
    assert_eq!(groups[0]["norm_hash"].as_str().map(str::len), Some(16));

    assert_eq!(
        repo.json(&["dupes", "--min-lines", "1", "--json"])
            .as_array()
            .map(Vec::len),
        Some(2),
        "the accessors are still clones, just filtered by default"
    );

    // A scope is a path prefix, and a group needs two members inside it.
    assert_eq!(
        repo.run(&["dupes", "app", "--json"]).1,
        1,
        "Delta is in lib"
    );
    assert_eq!(repo.run(&["dupes", "--json"]).1, 0);
}

/// The summarize loop, driven by replayed answers. No test may make a live API
/// call: a suite whose cost and result depend on the network is a suite nobody
/// runs.
#[test]
fn summarize_fills_a_budget_and_shares_clones() {
    // Two identical bodies in identical context (same name, same shape) and one
    // that differs, so the run exercises both the dedup and a real call.
    let body = "  def unpaid(customer)\n    invoices.where(customer: customer).unpaid\n  end\n";
    let repo = Repo::new(
        "summarize",
        &[
            ("a.rb", &format!("class Alpha\n{body}end\n")),
            ("b.rb", &format!("class Alpha\n{body}end\n")),
            (
                "c.rb",
                "class Beta\n  def total(order)\n    order.lines.sum(&:cents)\n  end\nend\n",
            ),
        ],
    );
    let fixtures = repo.dir.join("fixtures.json");
    std::fs::write(
        &fixtures,
        r#"{
          "Alpha#unpaid": {"summary":"Returns a customer's unpaid invoices.",
            "primary_purpose":"invoice lookup","secondary_concerns":[],
            "side_effects":[],"domain":"billing","patterns":["scope"]},
          "Beta#total": {"summary":"Totals an order's line items in cents.",
            "primary_purpose":"order total","secondary_concerns":[],
            "side_effects":[],"domain":"billing","patterns":[]}
        }"#,
    )
    .unwrap();
    repo.run(&["index"]);

    let fixtures = fixtures.to_str().unwrap();
    let filled = repo.json(&["summarize", "--fixtures", fixtures, "--json"]);
    // `a.rb` and `b.rb` hold the same body under the same name and owner, so
    // they are one purchase and one share — the dedup DEC-003 promises.
    assert_eq!(filled["summarized"], 2);
    assert_eq!(filled["shared"], 1);
    assert_eq!(filled["failed"], 0);
    assert_eq!(filled["remaining"], 0);

    // Coverage is per model, and counts every unit a stored answer serves.
    let status = repo.json(&["--status", "--json"]);
    let coverage = &status["checkouts"][0]["coverage"][0];
    assert_eq!(coverage["model"], "fixture");
    assert_eq!(coverage["state"], "complete");
    assert_eq!(coverage["summarized"], 3, "three units, two answers");
    assert_eq!(coverage["summarizable"], 3);

    // Re-running buys nothing: the answers are already stored.
    let again = repo.json(&["summarize", "--fixtures", fixtures, "--json"]);
    assert_eq!(again["summarized"], 0);
    assert_eq!(again["remaining"], 0);
}

/// A budget bounds spend, and a stale index refuses to summarize rather than
/// buying a wrong answer under a right key.
#[test]
fn summarize_respects_a_budget_and_refuses_stale_lines() {
    let method = |n: &str| {
        format!("class C{n}\n  def run{n}(a)\n    a.check\n    persist(a)\n    a\n  end\nend\n")
    };
    let repo = Repo::new(
        "budget",
        &[
            ("a.rb", &method("1")),
            ("b.rb", &method("2")),
            ("c.rb", &method("3")),
        ],
    );
    let fixtures = repo.dir.join("f.json");
    let one = r#"{"summary":"s","primary_purpose":"p","secondary_concerns":[],
                  "side_effects":[],"domain":"d","patterns":[]}"#;
    std::fs::write(
        &fixtures,
        format!(r#"{{"C1#run1": {one}, "C2#run2": {one}, "C3#run3": {one}}}"#),
    )
    .unwrap();
    repo.run(&["index"]);
    let fixtures = fixtures.to_str().unwrap();

    let first = repo.json(&[
        "summarize",
        "--budget",
        "2",
        "--fixtures",
        fixtures,
        "--json",
    ]);
    assert_eq!(first["summarized"], 2);
    assert_eq!(first["remaining"], 1, "the budget bounds spend, not scope");

    // Rewrite c.rb so the recorded span still exists and still parses, but now
    // holds a *different* method of the same shape. This is the dangerous
    // case: the context the index remembers (C3#run3) is unchanged, so the
    // request would look perfectly valid and store a wrong summary under the
    // right body's key — paid for, cached forever, indistinguishable from a
    // correct one. Only re-hashing the slice catches it.
    //
    // Deliberately the same line count as the original, so this fails if the
    // guard is weakened to a bounds check.
    std::fs::write(
        repo.dir.join("c.rb"),
        "class C3\n  def run3(a)\n    a.destroy\n    log(a)\n    nil\n  end\nend\n",
    )
    .unwrap();
    let stale = repo.json(&["summarize", "--fixtures", fixtures, "--json"]);
    assert_eq!(stale["summarized"], 0, "the body is not the one indexed");
    assert_eq!(stale["failed"], 1);
}
