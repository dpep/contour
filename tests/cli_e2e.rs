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
    /// `label` names this repo's temp directory and database, so it must be
    /// **unique across tests** — the suite runs them in parallel in one
    /// process, and `Drop` deletes the directory. Two repos sharing a label
    /// delete each other's files mid-run.
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

/// Fixtures for the search tests: two billing methods and one unrelated.
fn searchable(label: &str) -> (Repo, String) {
    let repo = Repo::new(
        label,
        &[
            (
                "billing.rb",
                "class Invoice\n  def unpaid_for(customer)\n    where(customer: customer, paid_at: nil).order(:created_at)\n  end\n\n  def settle!(at)\n    update!(paid_at: at)\n    Notifier.deliver(self)\n  end\nend\n",
            ),
            (
                "geometry.rb",
                "class Polygon\n  def area\n    vertices.each_cons(2).sum { |a, b| a.x * b.y - b.x * a.y } / 2.0\n  end\nend\n",
            ),
        ],
    );
    let path = repo.dir.join("f.json");
    std::fs::write(
        &path,
        r#"{
          "Invoice#unpaid_for": {"summary":"Returns the invoices a customer has not yet paid, oldest first.",
            "primary_purpose":"unpaid invoice lookup","secondary_concerns":["ordering"],
            "side_effects":[],"domain":"billing","patterns":["scope"]},
          "Invoice#settle!": {"summary":"Records payment on an invoice and notifies the customer.",
            "primary_purpose":"payment settlement","secondary_concerns":["notification"],
            "side_effects":["persists","network"],"domain":"billing","patterns":[]},
          "Polygon#area": {"summary":"Computes the enclosed area of a polygon from its vertices.",
            "primary_purpose":"area calculation","secondary_concerns":[],
            "side_effects":[],"domain":"geometry","patterns":["shoelace formula"]}
        }"#,
    )
    .unwrap();
    repo.run(&["index"]);
    (repo, path.to_str().unwrap().to_string())
}

/// Before anything is summarized, search still works — through the identifier
/// tier, which embeds what code is *called*. Zero LLM spend, and the answer
/// says which tier produced it so a reader knows what it could not see.
#[test]
fn search_works_with_no_summaries_through_the_identifier_tier() {
    let (repo, _) = searchable("search-names");
    let answer = repo.json(&["search", "unpaid", "--json"]);
    assert_eq!(answer["coverage_state"], "none", "nothing is summarized");
    assert_eq!(answer["hits"][0]["id"], "Invoice#unpaid_for");
    // Both halves reached it: the name matched lexically, and the embedded
    // identifier matched semantically.
    assert_eq!(answer["hits"][0]["how"], "both");
    assert_eq!(answer["hits"][0]["semantic_via"], "identifier");
    assert!(answer["hits"][0]["cosine"].as_f64().unwrap() > 0.0);
    // Every unit is covered by identifiers; none by summaries.
    assert_eq!(answer["tiers"]["summary"], 0);
    assert!(answer["tiers"]["identifier"].as_u64().unwrap() > 0);
}

/// With summaries in place, a query that names no identifier still finds the
/// method — which is the entire bet of the project (DEC-004).
#[test]
fn search_finds_by_meaning_once_summaries_exist() {
    let (repo, fixtures) = searchable("search-meaning");
    repo.run(&["summarize", "--fixtures", &fixtures]);

    let answer = repo.json(&["search", "customer has not paid", "--json"]);
    assert_eq!(answer["coverage_state"], "complete");
    assert_eq!(answer["coverage"]["summarized"], 3);
    assert_eq!(answer["embedder"], "hash");

    let top = &answer["hits"][0];
    assert_eq!(top["id"], "Invoice#unpaid_for");
    // The query shares no token with the method's name, so the semantic half
    // is what found it — and now through a summary rather than an identifier.
    assert_eq!(top["how"], "semantic");
    assert_eq!(top["semantic_via"], "summary");
    assert_eq!(
        answer["tiers"]["identifier"], 0,
        "summaries win where they exist"
    );
    assert!(top["cosine"].as_f64().unwrap() > 0.0);
    assert!(top["summary"].as_str().unwrap().contains("not yet paid"));

    // A query naming the method reaches it through both halves.
    let both = repo.json(&["search", "unpaid invoice lookup", "--json"]);
    assert_eq!(both["hits"][0]["how"], "both");
}

/// Nothing in the corpus answers this, so nothing should come back.
#[test]
fn search_can_return_nothing() {
    let (repo, fixtures) = searchable("search-nothing");
    repo.run(&["summarize", "--fixtures", &fixtures]);
    // The hash embedder has no calibrated floor, so set one explicitly: the
    // point being pinned is that a floor makes "no matches" reachable.
    let out = Command::new(env!("CARGO_BIN_EXE_contour"))
        .args(["search", "kubernetes scheduling affinity", "--json"])
        .current_dir(&repo.dir)
        .env("CONTOUR_DB", &repo.db)
        .env("CONTOUR_SEMANTIC_FLOOR", "0.9")
        .output()
        .unwrap();
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["hits"].as_array().map(Vec::len), Some(0));
    assert_eq!(answer["floor"], 0.9);
    assert_eq!(out.status.code(), Some(1), "a miss, not an error");
    // The floor is inherited from another corpus, so what it hid has to be
    // countable — otherwise it is a constant nobody can argue with.
    assert!(answer["withheld"].as_u64().unwrap() > 0);

    // `--floor 0` withholds nothing and answers from the whole ranking.
    let open = repo.json(&[
        "search",
        "kubernetes scheduling affinity",
        "--floor",
        "0",
        "--json",
    ]);
    assert_eq!(open["withheld"], 0);
    assert!(!open["hits"].as_array().unwrap().is_empty());
}

/// `similar` reports the tier that found each neighbour, and DEC-010's rule
/// about confidence: exact structural identity is a predicate and carries
/// evidence instead, while a cosine is graded and carries the number itself.
#[test]
fn similar_discloses_its_tier_and_only_grades_what_is_graded() {
    let body = "  def run(a)\n    a.validate\n    persist(a)\n    a\n  end\n";
    let repo = Repo::new(
        "similar",
        &[
            ("a.rb", &format!("class Alpha\n{body}end\n")),
            // Same body, different name and owner: an exact structural clone.
            (
                "b.rb",
                &format!("class Beta\n{}end\n", body.replace("run", "go")),
            ),
        ],
    );
    repo.run(&["index"]);

    let neighbors = repo.json(&["similar", "Alpha#run", "--json"]);
    let first = &neighbors[0];
    assert_eq!(first["id"], "Beta#go");
    assert_eq!(first["how"], "structural");
    assert!(
        first["confidence"].is_null(),
        "structural identity is a predicate, not a grade"
    );
    assert_eq!(first["lines"], 5, "it discloses evidence instead");

    assert_eq!(repo.run(&["similar", "Alpha#nope"]).1, 2, "no such unit");
}

/// The eval harness, end to end on the in-repo fixture corpus.
///
/// Asserts structure and corpus-level invariants, not scores: the numbers
/// depend on which embedder the build has, and pinning them would make this a
/// change-detector rather than a test. What must hold either way is that the
/// labels resolve, all three rankings run, and the labeled duplicates behave.
#[test]
fn eval_scores_a_labeled_set() {
    let set = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/eval/fixture");
    let corpus = set.join("corpus");
    let files: Vec<(String, String)> = std::fs::read_dir(&corpus)
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    let repo = Repo::new(
        "eval",
        &files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect::<Vec<_>>(),
    );
    repo.run(&["index"]);
    repo.run(&[
        "summarize",
        "--fixtures",
        set.join("summaries.json").to_str().unwrap(),
    ]);

    let set = set.to_str().unwrap();
    let report = repo.json(&["eval", set, "--min-lines", "1", "--json"]);

    // Every label names a unit that is actually in the index. A drifting
    // corpus that silently orphans labels is the way an eval starts lying.
    let contour = &report["rankings"][0];
    assert_eq!(contour["label"], "contour");
    assert_eq!(contour["unknown"], 0, "a label names a unit that is gone");
    assert_eq!(contour["total"], 22);
    // contour, contour:identifier, and the two baselines.
    assert_eq!(report["rankings"].as_array().map(Vec::len), Some(4));
    assert_eq!(report["rankings"][1]["label"], "contour:identifier");
    assert_eq!(report["coverage_state"], "complete");

    // The duplicate labels are the embedder-independent half: all three
    // copy-paste pairs collide, and not one of the near misses does.
    let dupes = &report["dupes"];
    assert_eq!(dupes["true_positives"], 3);
    assert_eq!(dupes["false_positives"], 0);
    assert_eq!(dupes["false_negatives"], 0);
    assert_eq!(dupes["unknown"], 0);

    // The sweep is the point of the exercise, so it must actually be there.
    let sweep = report["calibration"]["sweep"].as_array().unwrap();
    assert!(sweep.len() > 5);
    assert_eq!(sweep[0]["floor"], 0.0);
}
