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
    assert_eq!(status["coverage"], "none");
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
