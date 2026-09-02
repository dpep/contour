//! Nothing builds a thread pool for a checkout with nothing to parse.
//!
//! Its own test binary on purpose. `ThreadPoolBuilder::build_global` succeeds
//! exactly once per process, so the probe only means anything in a process
//! where no other test has touched rayon — and cargo runs a file's tests as
//! threads in one process.

use std::path::Path;
use std::process::Command;

/// Every command opens the index, and opening it refreshes it. On a checkout
/// where nothing moved there is nothing to parse, and the empty parallel map
/// that used to run there still built a thread per core and parked them — the
/// idle pool a field report watched through a stack sampler.
#[test]
fn refreshing_a_warm_checkout_builds_no_thread_pool() {
    let dir = std::env::temp_dir().join(format!("contour-pool-{}", std::process::id()));
    let db = dir.with_extension("db");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.rb"),
        "class Widget\n  def save\n    persist\n  end\nend\n",
    )
    .unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.email=t@e.st",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(&args)
                .current_dir(&dir)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    // The cold index — the run that has real blobs to parse — happens in
    // another process, so this one arrives at the warm refresh with rayon
    // untouched.
    let out = Command::new(env!("CARGO_BIN_EXE_contour"))
        .args(["index", dir.to_str().unwrap()])
        .env("CONTOUR_DB", &db)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");

    let mut store = contour::store::Store::open(&db).unwrap();
    let (_, indexed) = contour::index::index(&mut store, Path::new(&dir)).unwrap();
    assert_eq!(indexed.parsed, 0, "the refresh had nothing to parse");

    assert!(
        rayon::ThreadPoolBuilder::new().build_global().is_ok(),
        "a global rayon pool already existed, so something built one for no work"
    );

    let _ = std::fs::remove_dir_all(&dir);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
    }
}
