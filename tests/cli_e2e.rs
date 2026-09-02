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
        let (out, _, code) = self.run_in(&self.dir.clone(), args);
        (out, code)
    }

    /// The same binary and database, invoked from somewhere else. Only a test
    /// about *finding* the checkout needs this; everything else runs inside it.
    fn run_in(&self, cwd: &Path, args: &[&str]) -> (String, String, i32) {
        self.run_env(cwd, args, &[])
    }

    /// The same, with extra environment. Only the cost-budget cases need it.
    fn run_env(&self, cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_contour"))
            .args(args)
            .envs(env.iter().copied())
            .current_dir(cwd)
            .env("CONTOUR_DB", &self.db)
            // Hermetic: no case may consult whatever trekr or rq this machine
            // happens to have. Both external signals report themselves absent.
            .env("CONTOUR_TREKR", "/nonexistent/trekr")
            .env("CONTOUR_RQ", "/nonexistent/rq")
            .output()
            .expect("run contour");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code().unwrap_or(-1),
        )
    }

    /// The same, with something on stdin. `output()` gives a child a *null*
    /// stdin, so a command whose default input is a pipe needs its own runner
    /// or it is only ever tested through `--file`.
    fn run_stdin(&self, args: &[&str], input: &str) -> (String, String, i32) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contour"))
            .args(args)
            .current_dir(&self.dir)
            .env("CONTOUR_DB", &self.db)
            .env("CONTOUR_TREKR", "/nonexistent/trekr")
            .env("CONTOUR_RQ", "/nonexistent/rq")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("run contour");
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("wait for contour");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
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

/// `--status` reports on every checkout this machine has indexed; with a path
/// it reports on one. The index is per-machine, so on a working laptop the
/// unfiltered answer is mostly other people's repositories.
#[test]
fn status_narrows_to_the_checkout_you_name() {
    let here = Repo::new("status-here", &[("a.rb", "class A\n  def run; end\nend\n")]);
    let mut elsewhere = Repo::new(
        "status-elsewhere",
        &[("b.rb", "class B\n  def go; end\nend\n")],
    );
    // One database, two checkouts — the state `--status` exists to report on,
    // and the only one in which filtering can be wrong.
    elsewhere.db = here.db.clone();
    here.run(&["index"]);
    elsewhere.run(&["index"]);

    let all = here.json(&["--status", "--json"]);
    assert_eq!(all["checkouts"].as_array().map(Vec::len), Some(2));

    let one = here.json(&["--status", ".", "--json"]);
    assert_eq!(one["checkouts"].as_array().map(Vec::len), Some(1));
    assert!(
        one["checkouts"][0]["root"]
            .as_str()
            .is_some_and(|root| root.contains("status-here")),
        "got {one}"
    );

    // A checkout nothing has indexed is an empty answer rather than a failure:
    // the first query there will index it, and the line says so.
    let mut fresh = Repo::new("status-fresh", &[("c.rb", "class C\n  def x; end\nend\n")]);
    fresh.db = here.db.clone();
    let (_, err, code) = fresh.run_in(&fresh.dir.clone(), &["--status", "."]);
    assert_eq!(code, 1, "a miss, not an error");
    assert!(err.contains("not indexed"), "got {err:?}");
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

/// Staleness has to see the state a live session is always in: a tracked file
/// edited, nothing committed. The old probe stat'd `.git/index`, which an edit
/// does not touch, so `--status` said `stale: false` while `search` could not
/// find the method you had just written. Found by QA.
#[test]
fn staleness_sees_a_working_tree_edit_not_just_a_commit() {
    let repo = Repo::new(
        "stale",
        &[("a.rb", "class A\n  def one\n    1\n  end\nend\n")],
    );
    let stale = || repo.json(&["--status", "--json"])["checkouts"][0]["stale"] == true;

    repo.run(&["index"]);
    assert!(!stale(), "freshly indexed");

    std::fs::write(
        repo.dir.join("a.rb"),
        "class A\n  def one\n    1\n  end\n\n  def two\n    2\n  end\nend\n",
    )
    .unwrap();
    assert!(stale(), "a tracked file changed under us");
    repo.run(&["index"]);
    assert!(!stale(), "and reindexing settles it");

    // A brand-new untracked file is the other half git's index cannot see.
    std::fs::write(
        repo.dir.join("b.rb"),
        "class B\n  def three\n    3\n  end\nend\n",
    )
    .unwrap();
    assert!(stale(), "a new file is content the index does not hold");
    repo.run(&["index"]);
    assert!(!stale());

    // Committing changes no bytes, so it must not flip anything. The old probe
    // moved here and nowhere useful.
    git(&repo.dir, &["add", "-A"]);
    git(
        &repo.dir,
        &[
            "-c",
            "user.email=t@e.st",
            "-c",
            "user.name=test",
            "commit",
            "-qm",
            "later",
        ],
    );
    assert!(
        !stale(),
        "a commit of already-indexed bytes is not a change"
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
    // An object carrying the units and what the parse cost, exactly what the
    // MCP `symbols` tool returns for the same file.
    let outline = repo.json(&["--symbols", "a.rb", "--json"]);
    let rows = &outline["units"];
    assert_eq!(rows[0]["name"], "save");
    assert_eq!(rows[0]["owner"], "Widget");
    assert_eq!(rows[0]["params"][0]["kind"], "keyreq");
    assert_eq!(outline["parse_errors"], 0);

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

/// Two identical bodies that do different things, because each reads its own
/// `POLICY`.
///
/// rails' case in miniature: `Compatibility::V7_0#compatible_table_definition`
/// and its `V6_1` sibling are byte-identical, and each `TableDefinition`
/// resolves to its own version module's class. Consolidating them is not a
/// consolidation, so the group is reported with a caveat rather than dropped —
/// it is still two identical bodies, and the reader applies the rule.
///
/// `rq` is stubbed rather than called: the answer it gives is what decides the
/// caveat, so a case that depended on this machine's rq index would pass or
/// fail for reasons that have nothing to do with contour.
#[test]
fn dupes_caveats_a_group_whose_copies_read_different_constants() {
    let body = |ns: &str| {
        format!(
            "module {ns}\n  POLICY = :{ns}\n  class Runner\n    def apply(x)\n      \
             check(x)\n      POLICY\n    end\n  end\nend\n"
        )
    };
    let repo = Repo::new(
        "dupes-const",
        &[("app/v1.rb", &body("V1")), ("app/v2.rb", &body("V2"))],
    );
    repo.run(&["index"]);

    let stub = repo.dir.join("rq-stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\necho '[{\"name\":\"POLICY\",\"parent\":\"V1\"},\
         {\"name\":\"POLICY\",\"parent\":\"V2\"}]'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_contour"))
        .args(["dupes", "--json", "--min-lines", "3"])
        .current_dir(&repo.dir)
        .env("CONTOUR_DB", &repo.db)
        .env("CONTOUR_TREKR", "/nonexistent/trekr")
        .env("CONTOUR_RQ", &stub)
        .output()
        .expect("run contour");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let group = &report["groups"][0];
    assert_eq!(
        group["members"].as_array().map(Vec::len),
        Some(2),
        "{report}"
    );
    assert_eq!(group["caveat"]["constants"][0], "POLICY");
    assert!(
        group["caveat"]["basis"]
            .as_str()
            .is_some_and(|b| b.contains("POLICY")),
        "the basis names what to go and look at"
    );

    // Without rq the check cannot run, and saying nothing would crown a
    // consolidation that may not exist — so the run says it did not check.
    let (out, err, _) = repo.run_in(&repo.dir.clone(), &["dupes", "--min-lines", "3"]);
    assert!(!out.contains('!'), "no caveat claimed: {out}");
    assert!(err.contains("unchecked"), "{err}");
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
    let report = repo.json(&["dupes", "--json"]);
    let groups = &report["groups"];
    assert_eq!(groups.as_array().map(Vec::len), Some(1));
    assert_eq!(groups[0]["lines"], 5);
    assert_eq!(groups[0]["how"], "structural");
    // A u64 past 2^53 does not survive a JSON parser that stores numbers as
    // doubles, so the key travels as hex.
    assert_eq!(groups[0]["norm_hash"].as_str().map(str::len), Some(16));
    // Absolute, so a consumer can resolve it without knowing our cwd.
    assert!(
        groups[0]["members"][0]["path"]
            .as_str()
            .unwrap()
            .starts_with('/'),
        "{report}"
    );

    assert_eq!(
        repo.json(&["dupes", "--min-lines", "1", "--json"])["groups"]
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

/// The four findings behind DEC-022, as one checkout: a migration pair that
/// must not be offered for consolidation, a spec pair that must be offered but
/// apart, and an app pair that must lead the report.
#[test]
fn dupes_ignores_frozen_paths_and_ranks_test_code_apart() {
    // Four distinct shapes. The **spec** duplication is deliberately the
    // biggest payoff — a longer body copied three times — because that is the
    // case the ruling is about: ranked on `saves_nodes` alone it leads the
    // report, and a reader asking "what should I consolidate in this app" is
    // handed a spec helper.
    let app = "class %C%\n  def go(a)\n    b = a.one\n    c = b.two\n    save(c)\n  end\nend\n";
    let mixed = "class %C%\n  def take(a)\n    b = a.pick\n    store(b)\n  end\nend\n";
    let spec = "class %C%\n  def check(a)\n    b = a.first\n    c = b.second\n    d = c.third\n    e = d.fourth\n    expect(e).to be_ok\n  end\nend\n";
    let migration =
        "class %C%\n  def up\n    add_column :a, :b\n    add_index :a, :b\n  end\nend\n";
    let repo = Repo::new(
        "classes",
        &[
            ("app/a.rb", &app.replace("%C%", "Alpha")),
            ("app/b.rb", &app.replace("%C%", "Beta")),
            ("app/mine.rb", &mixed.replace("%C%", "Mine")),
            ("vendor/theirs.rb", &mixed.replace("%C%", "Theirs")),
            ("spec/a_spec.rb", &spec.replace("%C%", "AlphaSpec")),
            ("spec/b_spec.rb", &spec.replace("%C%", "BetaSpec")),
            ("spec/c_spec.rb", &spec.replace("%C%", "GammaSpec")),
            ("db/migrate/1_x.rb", &migration.replace("%C%", "AddX")),
            ("db/migrate/2_y.rb", &migration.replace("%C%", "AddY")),
        ],
    );
    repo.run(&["index"]);

    let report = repo.json(&["dupes", "--json"]);
    let classes: Vec<&str> = report["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .map(|g| g["class"].as_str().unwrap())
        .collect();
    // App code leads; the spec pair is reported but after it; the migration
    // pair is not offered for a consolidation that would rewrite the past.
    assert_eq!(classes, ["app", "mixed", "test"], "{report}");
    assert_eq!(report["groups"][2]["members"][0]["class"], "test");
    assert!(
        report["groups"][2]["saves_nodes"].as_u64() > report["groups"][0]["saves_nodes"].as_u64(),
        "the spec group must be the bigger payoff, or this proves nothing"
    );
    // A body that also exists in `vendor/` is a finding about the app copy, so
    // the group is kept and named for the disagreement.
    assert_eq!(report["groups"][1]["members"][1]["class"], "vendored");
    // The withholding is disclosed with its reason, not just its count.
    assert_eq!(report["withheld_paths"]["total"], 1);
    assert_eq!(report["withheld_paths"]["by_class"]["migration"], 1);

    let (_, stderr, _) = repo.run_in(&repo.dir.clone(), &["dupes"]);
    assert!(
        stderr.contains("1 group(s) in ignored paths withheld (1 migration)"),
        "{stderr}"
    );

    // And the default is overridable, which is the other half of the ruling.
    let all = repo.json(&["dupes", "--include-ignored", "--json"]);
    assert_eq!(all["groups"].as_array().map(Vec::len), Some(4));
    assert_eq!(all["withheld_paths"]["total"], 0, "nothing was withheld");
}

/// A repo whose layout differs says so, and is believed over the conventions.
#[test]
fn a_repo_config_overrides_the_conventions() {
    let body = "class %C%\n  def run(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "config",
        &[
            (
                ".contour.toml",
                "# This repo's migrations really are consolidatable.\n\
                 [paths]\napp = [\"db/migrate\"]\n",
            ),
            ("db/migrate/1_x.rb", &body.replace("%C%", "AddX")),
            ("db/migrate/2_y.rb", &body.replace("%C%", "AddY")),
        ],
    );
    repo.run(&["index"]);

    let report = repo.json(&["dupes", "--json"]);
    assert_eq!(report["groups"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["groups"][0]["class"], "app");
    assert_eq!(report["withheld_paths"]["total"], 0);

    // A config that says something wrong fails the run rather than being
    // half-applied — the same rule the eval's label vocabulary has.
    std::fs::write(
        repo.dir.join(".contour.toml"),
        "[paths]\nspecs = [\"spec\"]\n",
    )
    .unwrap();
    let (_, stderr, code) = repo.run_in(&repo.dir.clone(), &["dupes"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("`specs` is not a path class"), "{stderr}");
}

/// Finding the checkout is its own failure mode, and it used to surface as raw
/// git stderr from a command that had no way to be told where to look. Found
/// by running the Claude skill cold from outside a repository.
#[test]
fn similar_can_be_pointed_at_a_checkout_from_outside_it() {
    let body = "class %C%\n  def save(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "similar-path",
        &[
            ("app/a.rb", &body.replace("%C%", "Widget")),
            ("app/b.rb", &body.replace("%C%", "Gadget")),
        ],
    );
    repo.run(&["index"]);
    let outside = std::env::temp_dir();

    let (out, _, code) = repo.run_in(
        &outside,
        &["similar", "Widget#save", &repo.dir.to_string_lossy()],
    );
    assert_eq!(code, 0);
    assert!(out.contains("Gadget#save"), "got {out:?}");

    // With no path and nowhere to infer one from, the message names the real
    // problem rather than repeating git's.
    let (_, err, code) = repo.run_in(&outside, &["similar", "Widget#save"]);
    assert_eq!(code, 2);
    assert!(err.contains("not inside a git checkout"), "got {err:?}");
    assert!(!err.contains("fatal:"), "got {err:?}");
}

/// A name that means two units is refused, not quietly resolved to whichever
/// the index stored first — and the refusal says how to disambiguate. rails'
/// two `ConnectionPool::Wrapper#method_missing` defs are the real case: the
/// old answer printed the same id for the query and the result, so a reader
/// could not tell them apart. Found by QA.
#[test]
fn similar_refuses_an_ambiguous_name_and_takes_a_location() {
    let body = "    def method_missing(name, *args, &block)
      target = with_connection
                
      target.send(name, *args, &block)
    end
";
    let repo = Repo::new(
        "similar-ambiguous",
        &[(
            "pool.rb",
            &format!(
                "module ConnectionPool
  class Wrapper
{body}
    def other
      1
    end

{body}  end
end
"
            ),
        )],
    );
    repo.run(&["index"]);

    let (_, err, code) = repo.run_in(
        &repo.dir.clone(),
        &["similar", "ConnectionPool::Wrapper#method_missing"],
    );
    assert_eq!(code, 2, "ambiguity is an error, not a guess");
    assert!(err.contains("names 2 units"), "got {err:?}");
    assert!(
        err.contains("pool.rb:3") && err.contains("pool.rb:13"),
        "got {err:?}"
    );
    // The same message reaches an agent through the MCP tool, so it must not
    // instruct one in CLI syntax.
    assert!(!err.contains("contour similar"), "got {err:?}");

    // The location it just printed resolves, and finds the other copy as an
    // exact structural clone.
    let (out, code) = repo.run(&["similar", "pool.rb:3"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("pool.rb:13") && out.contains("[structural]"),
        "got {out:?}"
    );

    // No unit is reported twice under two tiers. The dedup used to compare
    // path strings, which stopped matching the moment JSON paths became
    // absolute — so the same body came back once as `structural` and again as
    // `semantic cos 1.00`, which is a structural fact wearing a graded number.
    let neighbors = repo.json(&["similar", "pool.rb:3", "--json"]);
    let listed: Vec<(&str, u64)> = neighbors["neighbors"]
        .as_array()
        .expect("an object with neighbours, like search's answer")
        .iter()
        .map(|n| (n["id"].as_str().unwrap(), n["line"].as_u64().unwrap()))
        .collect();
    let mut unique = listed.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(listed.len(), unique.len(), "reported twice: {listed:?}");
    // And the answer discloses what it could see, which it used to not do.
    assert!(neighbors["coverage_state"].is_string());
    assert!(neighbors["embedder"].is_string());
    // Paths a consumer can resolve without knowing where we were standing.
    assert!(
        neighbors["neighbors"][0]["path"]
            .as_str()
            .unwrap()
            .starts_with('/'),
        "{neighbors}"
    );

    // An unambiguous name is untouched.
    assert_eq!(repo.run(&["similar", "ConnectionPool::Wrapper#other"]).1, 0);
    // A location that names nothing is a miss with its own message.
    let (_, err, code) = repo.run_in(&repo.dir.clone(), &["similar", "pool.rb:999"]);
    assert_eq!(code, 2);
    assert!(err.contains("no unit at pool.rb:999"), "got {err:?}");
}

/// Every path that cannot be resolved says what is actually wrong, in the
/// house voice, without git's `fatal:` trailing behind it — and a path that
/// does not exist is named as that rather than as a repository question.
/// Found by QA across index/search/dupes/--symbols.
#[test]
fn a_path_that_cannot_be_resolved_names_the_real_problem() {
    let repo = Repo::new(
        "errors",
        &[("a.rb", "class A\n  def one\n    1\n  end\nend\n")],
    );
    repo.run(&["index"]);
    let outside = std::env::temp_dir();

    for args in [vec!["search", "anything"], vec!["dupes"], vec!["index"]] {
        let (_, err, code) = repo.run_in(&outside, &args);
        assert_eq!(code, 2, "{args:?}");
        assert!(
            err.contains("not inside a git checkout"),
            "{args:?}: {err:?}"
        );
        assert!(
            !err.contains("fatal:"),
            "{args:?} leaked git's stderr: {err:?}"
        );
    }

    // A nonexistent path is a different failure and gets a different sentence.
    let (_, err, _) = repo.run_in(&outside, &["search", "anything", "/nope/nothing"]);
    assert!(err.contains("/nope/nothing does not exist"), "{err:?}");
    // ...and the listing turns "no" into "here is what you probably meant".
    assert!(err.contains("contour has indexed:"), "{err:?}");

    let (_, err, _) = repo.run_in(&outside, &["--symbols", outside.to_str().unwrap()]);
    assert!(err.contains("is a directory"), "{err:?}");
    let (_, err, _) = repo.run_in(&outside, &["--symbols", "/nope/nothing.rb"]);
    assert!(err.contains("does not exist"), "{err:?}");
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

/// The fill loop re-parses each body to prove it is still the one the index
/// recorded. It used to do that as Ruby whatever the language, so every Rust
/// unit in a corpus was refused with "the file changed since it was indexed" —
/// a false claim about the user's files, and one the reindex it asks for
/// cannot fix. Found by QA against a Rust checkout.
#[test]
fn summarize_fills_both_languages_not_just_ruby() {
    let repo = Repo::new(
        "summarize-langs",
        &[
            (
                "app.rb",
                "class Widget
  def total(items)
    sum = 0
    items.each { |i| sum += i }
    sum
  end
end
",
            ),
            (
                "lib.rs",
                "impl Widget {
    fn total(&self, items: &[u8]) -> u32 {
        let mut sum = 0;
                 
        for i in items { sum += *i as u32; }
        sum
    }
}
",
            ),
        ],
    );
    repo.run(&["index"]);
    // Keyed the way each language's own surface names it — `Widget#total` in
    // Ruby, `Widget::total` in Rust. That the Rust key is not `Widget#total`
    // is half the point: the fixture path used to rebuild the id itself and
    // got Rust wrong.
    let fixtures = repo.dir.join("fx.json");
    std::fs::write(
        &fixtures,
        r#"{"Widget#total": {"summary":"sums numbers","primary_purpose":"aggregate",
             "secondary_concerns":[],"side_effects":[],"domain":"math","patterns":[]},
            "Widget::total": {"summary":"sums bytes","primary_purpose":"aggregate",
             "secondary_concerns":[],"side_effects":[],"domain":"math","patterns":[]}}"#,
    )
    .unwrap();

    let filled = repo.json(&[
        "summarize",
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(filled["summarized"], 2, "one per language");
    assert_eq!(filled["failed"], 0);
}

/// The purchased half has one door, and it is gated. `--fixtures` used to walk
/// an invented side effect straight into a table nothing ever drops, while the
/// MCP path rejected the identical payload. Found by QA.
#[test]
fn a_replayed_summary_is_gated_like_a_contributed_one() {
    let repo = Repo::new(
        "fixture-gate",
        &[(
            "app.rb",
            "class Widget
  def total(items)
    sum = 0
    items.each { |i| sum += i }
    sum
  end
             
  def label
    name.to_s.upcase.strip
  end
end
",
        )],
    );
    repo.run(&["index"]);
    let fixtures = repo.dir.join("fx.json");
    std::fs::write(
        &fixtures,
        r#"{"Widget#total": {"summary":"sums","primary_purpose":"aggregate",
             "secondary_concerns":[],"side_effects":["telepathy"],"domain":"math","patterns":[]},
            "Widget#label": {"summary":"formats a name","primary_purpose":"format",
             "secondary_concerns":[],"side_effects":["observes"],"domain":"display","patterns":[]}}"#,
    )
    .unwrap();

    let filled = repo.json(&[
        "summarize",
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--json",
    ]);
    // The invented vocabulary is refused; the good answer beside it is kept.
    // A batch that has already spent money must not be abandoned over one bad
    // answer, so this is a per-unit failure rather than a failed run.
    assert_eq!(filled["summarized"], 1);
    assert_eq!(filled["failed"], 1);
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
    // holds a *different* body under the same name. Every command that reads
    // the index refreshes it first, so the fill spends on the body that is
    // there now — where it used to refuse, having been asked about a body the
    // file no longer held. The guard that refuses is still in `fill::slice`,
    // for the race this cannot close; what changed is that the CLI no longer
    // walks into it.
    std::fs::write(
        repo.dir.join("c.rb"),
        "class C3\n  def run3(a)\n    a.destroy\n    log(a)\n    nil\n  end\nend\n",
    )
    .unwrap();
    let after = repo.json(&["summarize", "--fixtures", fixtures, "--json"]);
    assert_eq!(after["failed"], 0, "nothing was stale by the time it read");
    assert_eq!(after["summarized"], 1, "the body that is there now");
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

/// The berater complaint, as a test: a spec defines a helper with the obvious
/// name and outranks the method it tests. A discount, not an exclusion — the
/// spec is still an answer, just not the first one (DEC-022).
#[test]
fn search_ranks_a_spec_below_the_code_it_tests() {
    let repo = Repo::new(
        "search-classes",
        &[
            (
                "lib/limiter.rb",
                "class Limiter\n  def limit(key)\n    acquire(key)\n  end\nend\n",
            ),
            // Named so it sorts *before* the implementation: with no discount
            // the tie-break on path would hand it the top slot, which is what
            // makes the ranking assertion below fail when the discount is
            // removed rather than passing either way.
            (
                "a_limiter_spec.rb",
                "class LimiterSpec\n  def limit(key)\n    acquire(key)\n  end\nend\n",
            ),
            (
                "db/migrate/1_limits.rb",
                "class AddLimits\n  def limit(key)\n    acquire(key)\n  end\nend\n",
            ),
        ],
    );
    repo.run(&["index"]);

    let answer = repo.json(&["search", "limit", "--json"]);
    let ids: Vec<&str> = answer["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    // The spec is present — the ruling is explicit that ignoring tests is the
    // tempting wrong answer — and it is second.
    assert_eq!(ids, ["Limiter#limit", "LimiterSpec#limit"], "{answer}");
    assert_eq!(answer["hits"][0]["class"], "app");
    assert_eq!(answer["hits"][1]["class"], "test");
    // The discount is disclosed rather than being a silent thumb on the scale.
    assert_eq!(answer["discount"], 0.5);
    // Two decimals, and they survive serialization: rounding an f32 leaves an
    // f32, which widens back to a double on the way out, so a cosine reached
    // every agent as `0.44999998807907104` while the human output showed 0.45.
    // Rounded where the value is built, in the type it is built in.
    // Asserted on the rendered number, because that is what a consumer reads.
    let decimals = |value: &serde_json::Value| -> usize {
        let text = value.to_string();
        text.split_once('.').map_or(0, |(_, rest)| rest.len())
    };
    for hit in answer["hits"].as_array().unwrap() {
        assert!(
            decimals(&hit["cosine"]) <= 2,
            "{} claims more",
            hit["cosine"]
        );
    }
    assert!(decimals(&answer["floor"]) <= 2);
    assert!(
        answer["hits"][1]["score"].as_f64().unwrap() < answer["hits"][0]["score"].as_f64().unwrap()
    );
    // The migration is not an answer at all, and the count says so.
    assert_eq!(answer["withheld_paths"]["by_class"]["migration"], 1);

    let all = repo.json(&["search", "limit", "--include-ignored", "--json"]);
    assert_eq!(all["hits"].as_array().map(Vec::len), Some(3));
    assert_eq!(all["withheld_paths"]["total"], 0);
}

/// Delete a file and ask a question: the answer must not be about code that is
/// gone. Found by the owner on a scratch copy of berater — `--status` said
/// `[may be stale]` correctly, but no query consulted that probe, so `search`
/// still matched every unit of the deleted file. A thin answer looks thin; a
/// confidently wrong one does not.
#[test]
fn a_query_never_answers_from_a_stale_index() {
    let repo = Repo::new(
        "fresh",
        &[
            (
                "lib/keep.rb",
                "class Keep\n  def limit(a)\n    a\n  end\nend\n",
            ),
            (
                "lib/gone.rb",
                "class Gone\n  def limit(a)\n    a\n  end\nend\n",
            ),
        ],
    );
    repo.run(&["index"]);
    let ids = |repo: &Repo| -> Vec<String> {
        repo.json(&["search", "limit", "--json"])["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .map(|h| h["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(ids(&repo).contains(&"Gone#limit".to_string()));

    std::fs::remove_file(repo.dir.join("lib/gone.rb")).unwrap();
    let (_, stderr, _) = repo.run_in(&repo.dir.clone(), &["search", "limit"]);
    assert!(
        stderr.contains("index refreshed"),
        "the refresh is disclosed, not silent: {stderr}"
    );
    assert_eq!(ids(&repo), ["Keep#limit"], "the deleted method is gone");

    // `dupes` and `similar` answer from the same refreshed view, and `--status`
    // has nothing left to report.
    assert_eq!(repo.run(&["dupes", "--min-lines", "1"]).1, 1, "one is left");
    assert_eq!(
        repo.json(&["--status", "--json"])["checkouts"][0]["stale"],
        false
    );
}

/// Two runs of one query must agree. They did not: the semantic half is walked
/// out of a HashMap, so units with an equal cosine were handed RRF *ranks* in
/// map order, and the fused scores — and with them the answer — moved between
/// runs. Found while measuring a ranking change, which is exactly the work a
/// wandering baseline makes impossible.
#[test]
fn one_query_gives_one_answer() {
    let body = "class Limiter\n  def limit(key)\n    acquire(key)\n  end\nend\n";
    let repo = Repo::new(
        "search-stable",
        &[
            // Same owner and name in three files: identical identifier text,
            // so the cosines tie exactly and only the tie-break decides.
            ("lib/a.rb", body),
            ("lib/b.rb", body),
            ("lib/c.rb", body),
            ("lib/d.rb", &body.replace("Limiter", "Limits")),
        ],
    );
    repo.run(&["index"]);

    let order = || -> Vec<String> {
        repo.json(&["search", "limit", "--json"])["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .map(|h| format!("{}:{}", h["path"], h["score"]))
            .collect()
    };
    let first = order();
    assert!(first.len() >= 3, "{first:?}");
    for _ in 0..4 {
        assert_eq!(order(), first, "the same query answered differently");
    }
}

/// A summary outranks a name that merely looks like the query.
///
/// The field trial's finding, in miniature: on a partly-summarized rq the unit
/// whose contributed summary literally answered the question ranked **fifth**,
/// under four identifier-tier hits with long snake_case names that shared query
/// tokens by accident. RRF consumes a rank and discards the cosine, so a
/// summary hit and an identifier hit two places above it were worth almost the
/// same — and the whole grazing bargain (DEC-018) was invisible in the results.
#[test]
fn a_summary_outranks_a_name_that_only_looks_right() {
    let repo = Repo::new(
        "search-tiers",
        &[
            (
                "lib/support.rb",
                "class Support\n  def indexed(tag)\n    build(tag)\n  end\nend\n",
            ),
            // Nothing summarizes this one. Its long name shares more query
            // tokens by accident than the short name of the method that
            // actually does the thing — and it sorts first, so a tie would
            // hand it the top slot.
            (
                "lib/checks.rb",
                "class Checks\n  def indexing_prunes_a_stale_checkout_but_keeps_the_live_one\n    run\n  end\nend\n",
            ),
        ],
    );
    let fixtures = repo.dir.join("f.json");
    std::fs::write(
        &fixtures,
        r#"{"Support#indexed": {
             "summary":"Creates a throwaway repository for one test and indexes it into a fresh store.",
             "primary_purpose":"test fixture setup","secondary_concerns":[],
             "side_effects":["filesystem"],"domain":"testing","patterns":[]}}"#,
    )
    .unwrap();
    repo.run(&["index"]);
    repo.run(&["summarize", "--fixtures", fixtures.to_str().unwrap()]);

    let answer = repo.json(&[
        "search",
        // Both halves have something to say here: the long name shares "one"
        // and prefix-matches "index", so it leads the lexical list, while the
        // summary is what actually answers the question.
        "throwaway repository built for one test to index",
        "--json",
    ]);
    assert_eq!(answer["coverage_state"], "warming", "one of two summarized");
    let top = &answer["hits"][0];
    assert_eq!(top["id"], "Support#indexed", "{answer}");
    assert_eq!(top["semantic_via"], "summary");
    // The name-shaped hit is still an answer, just not the first one.
    assert_eq!(
        answer["hits"][1]["id"],
        "Checks#indexing_prunes_a_stale_checkout_but_keeps_the_live_one"
    );
    assert_eq!(answer["hits"][1]["semantic_via"], "identifier");
}

/// One filler word shared with the query is not worth a place in the ranking.
///
/// M12b's repro, in miniature and with its own phrasing. `Class#is_app` shares
/// exactly `is` with a ten-word question and nothing else; the unit whose
/// summary answers it shares no word at all. RRF consumes a rank and discards
/// the score, so being *in* the lexical list at all used to be worth a full
/// `1/(K+1)` — and that plus an identifier-tier cosine beat a summary that
/// answered the question outright.
///
/// **This test fails if the lexical half's weight is dropped back to 1.0**,
/// which is the whole of DEC-027.
#[test]
fn a_name_that_shares_one_filler_word_does_not_outrank_the_answer() {
    let repo = Repo::new(
        "search-filler",
        &[
            // Sorts first, so a near-tie hands it the top slot.
            (
                "lib/checks.rb",
                "class Class\n  def is_app\n    true\n  end\nend\n",
            ),
            (
                "lib/server.rb",
                "class Server\n  def superseded\n    stat\n  end\nend\n",
            ),
        ],
    );
    let fixtures = repo.dir.join("f.json");
    std::fs::write(
        &fixtures,
        r#"{"Server#superseded": {
             "summary":"Notices that the program running is no longer the one on disk and returns what replaced it.",
             "primary_purpose":"detect a replaced binary","secondary_concerns":[],
             "side_effects":[],"domain":"process","patterns":[]}}"#,
    )
    .unwrap();
    repo.run(&["index"]);
    repo.run(&["summarize", "--fixtures", fixtures.to_str().unwrap()]);

    let answer = repo.json(&[
        "search",
        "notice the program on disk is not the one running",
        "--json",
    ]);
    let top = &answer["hits"][0];
    assert_eq!(top["id"], "Server#superseded", "{answer}");
    assert_eq!(top["semantic_via"], "summary");

    // The filler match is still an answer, and now says how little it matched:
    // one word of ten, which is what `how: both` alone could never convey.
    let filler = answer["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == "Class#is_app")
        .unwrap_or_else(|| panic!("{answer}"));
    assert_eq!(filler["how"], "both");
    assert_eq!(filler["lexical"], 0.1);
}

/// A class answers for the one method a caller can reach.
///
/// M12b's census, in miniature. `Archiver#call` is named for the protocol it
/// implements and says nothing about backups; its private helpers are where
/// the behaviour is written down. The container's centroid is the mean of
/// them, so the class matches a query none of its entry point's own words do,
/// and nominates the one unit a caller could actually call.
///
/// The negatives are the same fixture with the rule denied: give the class a
/// second public method and it must go silent rather than guess.
#[test]
fn a_class_answers_for_its_one_public_method() {
    let sole = "class Archiver\n  def call\n    build_zip_of_media\n  end\n  private\n  \
                def build_zip_of_media; end\n  def dump_outbox_json; end\nend\n";
    let repo = Repo::new("nominate", &[("lib/archiver.rb", sole)]);
    repo.run(&["index"]);
    let answer = repo.json(&["search", "build a zip archive of media", "--json"]);

    let hit = answer["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == "Archiver#call")
        .unwrap_or_else(|| panic!("{answer}"));
    let nomination = &hit["nominated"];
    assert_eq!(nomination["container"], "Archiver");
    assert_eq!(nomination["rule"], "its container's only public unit");
    // The measurement that earned it, disclosed like every other (DEC-010).
    assert!(nomination["cosine"].as_f64().unwrap() > 0.0, "{answer}");

    // Two public methods: no single front door, so no nomination. Abstaining
    // is the answer, not a pick between them.
    let two = sole.replace("  private\n", "");
    let repo = Repo::new("nominate-two", &[("lib/archiver.rb", &two)]);
    repo.run(&["index"]);
    let answer = repo.json(&["search", "build a zip archive of media", "--json"]);
    for hit in answer["hits"].as_array().unwrap() {
        assert!(hit["nominated"].is_null(), "{hit}");
    }

    // An `attr_reader` is declared, not written, and must not count as a
    // second public method — Rails classes carry them routinely, and counting
    // them silenced every service object contour was built to find.
    let accessor = sole.replace("class Archiver\n", "class Archiver\n  attr_reader :log\n");
    let repo = Repo::new("nominate-attr", &[("lib/archiver.rb", &accessor)]);
    repo.run(&["index"]);
    let answer = repo.json(&["search", "build a zip archive of media", "--json"]);
    let hit = answer["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == "Archiver#call")
        .unwrap_or_else(|| panic!("{answer}"));
    assert_eq!(hit["nominated"]["container"], "Archiver");
}

/// One rule for what kind of code a unit is, on every surface that says so.
///
/// A Rust `#[cfg(test)] mod tests` sits inside the file it tests, so the file
/// is app code and the units in that module are not (DEC-022's standing
/// example). `search` has discounted them per unit since M11c; `dupes` and
/// `similar` were still asking the *path*, so the same unit came back tagged
/// `app` from one command and `test` from another.
#[test]
fn every_surface_agrees_what_kind_of_code_a_unit_is() {
    let body = "fn helper(n: u8) -> u8 {\n    n + 1\n}\n\n#[cfg(test)]\nmod tests {\n    \
                fn helper(n: u8) -> u8 {\n        n + 1\n    }\n}\n";
    let repo = Repo::new("of-unit", &[("src/lib.rs", body)]);
    repo.run(&["index"]);

    // The inline test's twin is a duplicate of the production function, and
    // the group has to say that one copy is test code.
    let groups = repo.json(&["dupes", "--json", "--min-lines", "1"]);
    let members = &groups["groups"][0]["members"];
    let classes: Vec<&str> = members
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["class"].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"test"), "{groups}");
    assert!(classes.contains(&"app"), "{groups}");

    // `similar` reports the same unit and must reach the same verdict.
    let neighbours = repo.json(&["similar", "helper", "--json"]);
    let tagged: Vec<&str> = neighbours["neighbors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["class"].as_str().unwrap())
        .collect();
    assert!(tagged.contains(&"test"), "{neighbours}");
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
            // A third copy, vendored. Identical, and still not an answer to
            // "has this been written before" — nobody consolidates a copy of
            // somebody else's gem (DEC-022).
            (
                "vendor/c.rb",
                &format!("class Gamma\n{}end\n", body.replace("run", "spin")),
            ),
        ],
    );
    repo.run(&["index"]);

    let answer = repo.json(&["similar", "Alpha#run", "--json"]);
    let first = &answer["neighbors"][0];
    assert_eq!(first["id"], "Beta#go");
    assert_eq!(first["how"], "structural");
    assert_eq!(first["class"], "app");
    assert!(
        first["cosine"].is_null() && first["similarity"].is_null(),
        "structural identity is a predicate, not a grade"
    );
    assert_eq!(first["lines"], 5, "it discloses evidence instead");

    let ids: Vec<&str> = answer["neighbors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["Beta#go"], "the vendored clone is not offered");
    assert_eq!(answer["withheld_paths"]["by_class"]["vendored"], 1);
    // Asked for, it comes back tagged — the default is disclosed and
    // overridable, never silent.
    let all = repo.json(&["similar", "Alpha#run", "--include-ignored", "--json"]);
    assert_eq!(all["neighbors"][1]["id"], "Gamma#spin");
    assert_eq!(all["neighbors"][1]["class"], "vendored");

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
    assert_eq!(contour["total"], 23);
    // contour, contour:identifier, the natural-phrasing band, and the two
    // baselines. The band is its own row and its own population — same 23
    // expected answers, asked the way a person types them.
    assert_eq!(report["rankings"].as_array().map(Vec::len), Some(5));
    assert_eq!(report["rankings"][1]["label"], "contour:identifier");
    assert_eq!(report["rankings"][2]["label"], "contour:natural");
    assert_eq!(report["rankings"][2]["total"], 23);
    assert_eq!(
        report["rankings"][2]["unknown"], 0,
        "a natural label is orphaned"
    );
    assert_eq!(report["coverage_state"], "complete");

    // The duplicate labels are the embedder-independent half: all three
    // copy-paste pairs collide, and not one of the near misses does.
    let dupes = &report["dupes"];
    assert_eq!(dupes["true_positives"], 3);
    assert_eq!(dupes["false_positives"], 0);
    assert_eq!(dupes["false_negatives"], 0);
    assert_eq!(dupes["unknown"], 0);

    // `similar` is scored by running the tool exactly as a caller would, so
    // this is the one assertion that would catch the neighbour list quietly
    // changing tier — the claim, not just the membership.
    let sim = &report["similar"];
    assert_eq!(sim["correct"], 3, "{sim}");
    assert_eq!(sim["wrong"], 0, "{sim}");
    assert_eq!(sim["unknown"], 0, "a label names a unit that is gone");

    // The sweep is the point of the exercise, so it must actually be there.
    let sweep = report["calibration"]["sweep"].as_array().unwrap();
    assert!(sweep.len() > 5);
    assert_eq!(sweep[0]["floor"], 0.0);
}

/// Grazing, from a command line. Two field trials asked for this
/// independently, for the same reason: a resident MCP server can be the wrong
/// build for a whole session, and when it is, a command is the only way left to
/// contribute anything.
///
/// The round trip is what this pins — `pending` offers a unit, a summary goes
/// back in, and `pending` stops offering it. The gate matrix belongs to
/// `mcp_e2e`, because both doors are one function and testing it twice would
/// only prove that.
#[test]
fn a_summary_can_be_contributed_from_the_command_line() {
    let repo = Repo::new(
        "graze",
        &[(
            "billing.rb",
            "class Invoice\n  def unpaid_for(customer)\n    where(customer: customer, paid_at: nil).order(:created_at)\n  end\nend\n",
        )],
    );

    let offered = repo.json(&["pending", "--model", "m", "--json"]);
    assert_eq!(offered["prompt_version"], "mcp-v1");
    assert_eq!(offered["units"].as_array().map(Vec::len), Some(1));
    let unit = &offered["units"][0];
    assert_eq!(unit["id"], "Invoice#unpaid_for");
    // The source and the context are the point of `pending`: a session that has
    // to open the file first has been told nothing it did not know.
    assert!(
        unit["source"].as_str().is_some_and(|s| s.contains("def ")),
        "got {unit}"
    );
    assert!(
        unit["context"]
            .as_str()
            .is_some_and(|c| c.contains("defined on: Invoice")),
        "got {unit}"
    );

    let payload = serde_json::json!({
        "unit": "Invoice#unpaid_for",
        "model": "m",
        "prompt_version": offered["prompt_version"],
        "summary": {
            "summary": "Returns a customer's unpaid invoices, oldest first.",
            "primary_purpose": "unpaid invoice lookup",
            "secondary_concerns": ["ordering"],
            "side_effects": ["persists"],
            "domain": "billing",
            "patterns": ["scope"]
        }
    })
    .to_string();

    let (out, _, code) = repo.run_stdin(&["store-summary", "--json"], &payload);
    assert_eq!(code, 0, "got {out}");
    let stored: serde_json::Value = serde_json::from_str(&out).expect("accepted, as JSON");
    assert_eq!(stored["id"], "Invoice#unpaid_for");
    // Contributions key apart from an API fill (DEC-018), whichever door they
    // come through: `via` is who paid, not which transport carried it.
    assert_eq!(stored["via"], "mcp");

    // The whole point: what a session contributes, the next one does not have
    // to buy again.
    let after = repo.json(&["pending", "--model", "m", "--json"]);
    assert_eq!(after["units"].as_array().map(Vec::len), Some(0));
    assert_eq!(repo.run(&["pending", "--model", "m"]).1, 1, "nothing left");

    // Rejected rather than repaired, on this door as on the other — and the
    // refusal names the field, because a payload typed against the CLI has no
    // schema steering it the way an MCP client's does.
    let flat = serde_json::json!({
        "unit": "Invoice#unpaid_for", "model": "m", "prompt_version": "mcp-v1",
        "summary": "Returns a customer's unpaid invoices."
    })
    .to_string();
    let (_, err, code) = repo.run_stdin(&["store-summary"], &flat);
    assert_eq!(code, 2);
    assert!(err.contains("`summary` must be an object"), "got {err:?}");
}

/// Two builds of one commit answer English differently and look identical on
/// disk: a default build matches names, a `semantic` build matches meaning.
/// `search` disclosed which embedder answered, but only after you had run one —
/// and a reinstall with the feature flag left off downgrades an index silently.
/// So the two commands somebody runs *before* asking a question say it too.
#[test]
fn the_binary_says_which_embedder_it_was_built_with() {
    let repo = Repo::new("build-info", &[("a.rb", "class A\n  def run; end\nend\n")]);

    let (version, code) = repo.run(&["--version"]);
    assert_eq!(code, 0);
    assert!(version.contains("embedder:"), "got {version:?}");

    repo.run(&["index"]);
    let status = repo.json(&["--status", "--json"]);
    assert!(
        status["build"]
            .as_str()
            .is_some_and(|b| b.contains("embedder")),
        "got {status}"
    );
    // The same fact in both places, because a reader who checked one should not
    // have to wonder whether the other disagrees.
    assert!(
        version.contains(status["build"].as_str().unwrap()),
        "got {version:?}"
    );
}

/// `similar`'s second positional is a scope, the same noun `search` and
/// `dupes` take there: the working directory or the path you name bounds the
/// *answers*, while the unit asked about is found wherever it lives.
///
/// Field-reported against a monorepo: `similar` had no way to say "look here",
/// so every call had to hold a vector for every unit in the checkout.
#[test]
fn similar_takes_a_scope_and_says_which_one_it_searched() {
    let body = "class %C%\n  def save(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "similar-scope",
        &[
            ("a/one.rb", &body.replace("%C%", "Widget")),
            ("b/two.rb", &body.replace("%C%", "Gadget")),
        ],
    );
    repo.run(&["index"]);

    let all = repo.json(&["similar", "Widget#save", "--json"]);
    assert_eq!(all["neighbors"][0]["id"], "Gadget#save");
    assert!(all["scope"].is_null(), "the whole checkout names no scope");

    // The clone is in `b`, so a scope of `a` holds nothing to find — and the
    // run says where it looked, or an empty answer reads as a thin corpus.
    let (_, err, code) = repo.run_in(&repo.dir.clone(), &["similar", "Widget#save", "a"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("under a only"), "got {err:?}");

    // The unit itself is in `a`, outside this scope: a narrower search, not a
    // missing unit.
    let elsewhere = repo.json(&["similar", "Widget#save", "b", "--json"]);
    assert_eq!(elsewhere["scope"], "b");
    assert_eq!(elsewhere["neighbors"][0]["id"], "Gadget#save");

    // Standing in a directory scopes to it, as it already does for `search`
    // and `dupes` — the rule is one rule, and it is now disclosed.
    let inside = repo.dir.join("a");
    let (out, _, code) = repo.run_in(&inside, &["similar", "Widget#save", "--json"]);
    assert_eq!(code, 1, "{out}");
    let narrowed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(narrowed["scope"], "a");
    // The point of the scope, stated as a number: the units a call has to
    // hold a vector for are the ones inside it.
    assert_eq!(narrowed["coverage"]["summarizable"], 1);
    assert_eq!(all["coverage"]["summarizable"], 2);
}

/// A cold corpus too big to embed inside the budget is refused before any of
/// it is paid for, and the refusal carries the numbers it was refused on.
///
/// The field report: an unscoped call on ~2M units ran 20+ minutes with no
/// estimate and no way to bound it. Measured, that call is a corpus-sized
/// inference run — about two hours — and the useful thing to hand back is not
/// a progress bar but the bill and a smaller question.
///
/// **The budget in this test is microseconds, and it has to be**: the hash
/// embedder this build carries does about 8.5 million texts a second, so
/// nothing a test can generate costs a readable number of them. What is pinned
/// here is the plumbing — estimate, refusal, message, override. The rates
/// behind the estimate are measurements, recorded in `docs/PLAN.md`.
#[test]
fn an_unaffordable_cold_corpus_is_refused_with_its_bill() {
    let body = "class %C%\n  def save(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "budget",
        &[
            ("a/one.rb", &body.replace("%C%", "Widget")),
            ("b/two.rb", &body.replace("%C%", "Gadget")),
        ],
    );
    repo.run(&["index"]);

    let tiny = [("CONTOUR_EMBED_BUDGET", "0.0000000001")];
    let (_, err, code) = repo.run_env(&repo.dir.clone(), &["search", "persist a thing"], &tiny);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("2 unit(s)"), "the scope's size: {err:?}");
    assert!(err.contains("nothing embedded yet"), "{err:?}");
    assert!(err.contains("Narrow the scope"), "{err:?}");
    assert!(err.contains("CONTOUR_EMBED_BUDGET"), "{err:?}");

    // `similar` goes through the same gate, and a scope is the answer the
    // message points at.
    let (_, err, code) = repo.run_env(&repo.dir.clone(), &["similar", "Widget#save"], &tiny);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("1 unit(s)") || err.contains("2 unit(s)"),
        "{err:?}"
    );

    // Nothing was embedded and nothing was written, so removing the budget
    // still finds the same answer.
    let (_, err, code) = repo.run_env(
        &repo.dir.clone(),
        &["search", "persist a thing"],
        &[("CONTOUR_EMBED_BUDGET", "0")],
    );
    assert_eq!(code, 0, "{err}");
}

/// The refusal's way out (DEC-034). `embed` is the consent DEC-032's message
/// asks for, so it never refuses, and once it has run the same query under the
/// same budget answers — which is the whole "refused → embed once → warm
/// forever" story, end to end.
///
/// The budget here is microseconds for the reason the case above gives: this
/// build's hash embedder is too fast for a test to spend a readable amount of
/// time. What is pinned is the plumbing.
#[test]
fn a_filled_scope_answers_the_query_it_refused_cold() {
    let body = "class %C%\n  def save(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let repo = Repo::new(
        "embed-fill",
        &[
            ("a/one.rb", &body.replace("%C%", "Widget")),
            ("b/two.rb", &body.replace("%C%", "Gadget")),
        ],
    );
    repo.run(&["index"]);
    let tiny = [("CONTOUR_EMBED_BUDGET", "0.0000000001")];
    let here = repo.dir.clone();

    let (_, err, code) = repo.run_env(&here, &["search", "persist a thing"], &tiny);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("contour embed"), "the way out: {err:?}");

    // The same budget, and this one pays rather than refusing.
    let (out, err, code) = repo.run_env(&here, &["embed", "-j"], &tiny);
    assert_eq!(code, 0, "{err}");
    let filled: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(filled["units"], 2);
    assert!(filled["embedded"].as_u64().unwrap() > 0, "{out}");
    assert_eq!(filled["remaining"], 0);
    assert_eq!(filled["embedder"], "hash");
    assert!(err.contains("text(s) to embed"), "the estimate: {err:?}");

    // Warm, under the budget that refused it.
    let (out, err, code) = repo.run_env(&here, &["search", "persist a thing"], &tiny);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Widget#save"), "{out}");

    // A warm scope is a fill with nothing to do, which is the house's
    // "nothing happened" exit — and one compact line under --ndjson.
    let (out, err, code) = repo.run_env(&here, &["embed", "-J"], &tiny);
    assert_eq!(code, 1, "{err}");
    assert_eq!(out.lines().count(), 1, "{out}");
    let again: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(again["embedded"], 0);
    assert_eq!(again["warm"], filled["texts"]);

    // A budget is time to spend, so zero is a mistake rather than a synonym
    // for the environment variable's "no budget".
    let (_, err, code) = repo.run_env(&here, &["embed", "--budget", "0"], &tiny);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("more than zero"), "{err:?}");
}

/// `--profile` is an instrument, so what it must never do is change the
/// answer: the phases go to stderr, stdout stays exactly the data, and the
/// exit code is the one the run earned.
///
/// The field report that asked for this had to attach an OS stack sampler to a
/// release binary to learn where a query's time went. What it needed was for
/// the reads to be named and for the part nobody had named to be visible,
/// which is what `unaccounted` is.
#[test]
fn profile_reports_phases_on_stderr_without_touching_the_answer() {
    let (repo, _) = searchable("profile-phases");

    let (plain, code) = repo.run(&["search", "unpaid", "--json"]);
    let (out, err, profiled) = repo.run_in(
        &repo.dir.clone(),
        &["search", "unpaid", "--json", "--profile"],
    );
    assert_eq!(out, plain, "the profile must not reach stdout");
    assert_eq!(profiled, code, "nor change the exit code");

    // With a JSON format asked for, the profile is one compact object.
    let report: serde_json::Value =
        serde_json::from_str(err.lines().last().unwrap()).expect("a JSON profile on stderr");
    assert!(report["total_ms"].as_f64().unwrap() > 0.0);
    assert!(report["unaccounted_ms"].is_number());
    let named: Vec<&str> = report["phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    for phase in ["store open", "read units", "score", "render"] {
        assert!(named.contains(&phase), "{phase} missing from {named:?}");
    }
    assert!(report["counters"]["unit rows read"].as_u64().unwrap() > 0);

    // Human format gets the table, and it accounts for the whole run.
    let (_, human, _) = repo.run_in(&repo.dir.clone(), &["search", "unpaid", "--profile"]);
    assert!(human.contains("unaccounted"), "{human}");
    assert!(human.contains("total"), "{human}");

    // Off by default: no phase table, no JSON object.
    let (_, quiet, _) = repo.run_in(&repo.dir.clone(), &["search", "unpaid"]);
    assert!(!quiet.contains("unaccounted"), "{quiet}");
}

/// A profile describes one run, and the server answers many at once — so the
/// combination is refused rather than printing shares that overlap.
#[test]
fn profile_is_refused_for_the_server() {
    let repo = Repo::new(
        "profile-mcp",
        &[("a.rb", "class A\n  def b\n    1\n  end\nend\n")],
    );
    let (_, err, code) = repo.run_in(&repo.dir.clone(), &["mcp", "--profile"]);
    assert_eq!(code, 2);
    assert!(err.contains("one run"), "{err}");
}

/// `similar` reads the checkout's unit table once.
///
/// It used to read it twice — once for the units it ranks, once inside the
/// near-structural tier, which fetched and re-filtered the rows its only
/// caller was already holding. On a monorepo that second read was the single
/// largest phase of a warm scoped query. The assertion is on the profile
/// counter rather than on the answer, because the answer never changed: this
/// is the one claim a behavioural test cannot make.
#[test]
fn similar_reads_the_unit_table_once() {
    let (repo, _) = searchable("similar-one-read");
    let (_, err, _) = repo.run_in(
        &repo.dir.clone(),
        &["similar", "Invoice#settle!", "--json", "--profile"],
    );
    let report: serde_json::Value =
        serde_json::from_str(err.lines().last().unwrap()).expect("a JSON profile on stderr");
    let reads = report["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "read units")
        .expect("the unit read is a phase");
    assert_eq!(reads["times"], 1, "{report}");
    // Three units in the fixture, read once — not six.
    assert_eq!(report["counters"]["unit rows read"], 3, "{report}");
}

/// The field case's shape, mirrored: a one-letter word in the question must
/// not decide the ranking.
///
/// The corpus it was reported on is private, so what is pinned here is the
/// mechanism rather than the case — a guard clause and a method that calls it,
/// where the caller's name begins with the query's `a`. Prefix credit for that
/// `a` used to buy the caller a second term in the fusion, which the guard did
/// not have, and it won on the tie.
#[test]
fn a_one_letter_query_word_does_not_decide_the_ranking() {
    let repo = Repo::new(
        "short-token",
        &[(
            "shipping.rb",
            "class Shipment\n\
             \x20 def ensure_open!\n\
             \x20   raise Frozen unless open?\n\
             \x20 end\n\n\
             \x20 def add_parcel(parcel)\n\
             \x20   ensure_open!\n\
             \x20   parcels << parcel\n\
             \x20 end\n\
             end\n",
        )],
    );
    let fixtures = repo.dir.join("f.json");
    std::fs::write(
        &fixtures,
        r#"{
          "Shipment#ensure_open!": {"summary":"Refuses any change to a shipment that has already been finalized.",
            "primary_purpose":"finalized shipment guard","secondary_concerns":[],
            "side_effects":["raises"],"domain":"shipping","patterns":["guard clause"]},
          "Shipment#add_parcel": {"summary":"Adds a parcel to a shipment, refusing once the shipment has been finalized.",
            "primary_purpose":"parcel addition","secondary_concerns":["guarding"],
            "side_effects":["mutates"],"domain":"shipping","patterns":[]}
        }"#,
    )
    .unwrap();
    repo.run(&["index"]);
    repo.run(&["summarize", "--fixtures", fixtures.to_str().unwrap()]);

    let answer = repo.json(&["search", "prevent a change once finalized", "--json"]);
    // The mechanism, not just the outcome: nothing in this question matches a
    // name, so no hit may claim a lexical half at all. Before the fix
    // `add_parcel` claimed one, on the strength of `a`.
    for hit in answer["hits"].as_array().unwrap() {
        assert!(hit["lexical"].is_null(), "{hit}");
        assert_eq!(hit["how"], "semantic", "{hit}");
    }
    assert_eq!(answer["hits"][0]["id"], "Shipment#ensure_open!", "{answer}");
}

/// `duplicates` is the same command as `dupes`, spelled out.
///
/// An alias rather than a rename: `dupes` is what the README, the skill and
/// the MCP tool all say, and two names that mean two things is the cost of
/// getting that wrong. Not added to the MCP surface — a model picks from
/// `tools/list`, so a second name there is one more thing to choose between
/// and nothing a client could have typed.
#[test]
fn duplicates_is_the_long_spelling_of_dupes() {
    let body =
        "class Widget\n  def save\n    a = compute\n    b = a + 1\n    persist(b)\n  end\nend\n";
    let twin = body.replace("Widget", "Gadget").replace("save", "store");
    let repo = Repo::new("dupes-alias", &[("a.rb", body), ("b.rb", &twin)]);
    repo.run(&["index"]);

    let short = repo.json(&["dupes", "--min-lines", "1", "--json"]);
    let long = repo.json(&["duplicates", "--min-lines", "1", "--json"]);
    assert_eq!(short, long);
    assert_eq!(short["groups"].as_array().map(Vec::len), Some(1), "{short}");
}
