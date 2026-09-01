//! A real stdio session against the built binary.
//!
//! The lesson that earned this file: in Phase 1.5 both extractors were
//! individually correct and unit-tested, and the command that used them was
//! wired to the wrong one. Every piece here has its own unit test too; only
//! driving the actual process proves they are connected.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

/// A live `contour mcp` process, driven the way a client drives one.
struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    #[allow(dead_code)]
    dir: PathBuf,
    db: PathBuf,
}

impl Session {
    fn start(label: &str, files: &[(&str, &str)]) -> Session {
        Session::start_using(label, files, Path::new(env!("CARGO_BIN_EXE_contour")), &[])
    }

    /// The same, with extra environment. Only the cost-budget case needs it.
    fn start_with_env(label: &str, files: &[(&str, &str)], env: &[(&str, &str)]) -> Session {
        Session::start_using(label, files, Path::new(env!("CARGO_BIN_EXE_contour")), env)
    }

    /// The same session, served by a named program rather than by the build
    /// under test. Only the upgrade case needs this: it launches from a *copy*
    /// so the copy can be replaced underneath the running process.
    fn start_using(
        label: &str,
        files: &[(&str, &str)],
        program: &Path,
        env: &[(&str, &str)],
    ) -> Session {
        let base = std::env::temp_dir();
        let dir = base.join(format!("contour-mcp-{}-{label}", std::process::id()));
        let db = base.join(format!("contour-mcp-{}-{label}.db", std::process::id()));
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
                "m",
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

        let mut child = Command::new(program)
            .arg("mcp")
            .envs(env.iter().copied())
            .current_dir(&dir)
            .env("CONTOUR_DB", &db)
            // Hermetic: a test must not shell out to whatever trekr this
            // machine has, or index a temp repo into its global store. An
            // unrunnable path is also the degraded path worth exercising.
            .env("CONTOUR_TREKR", "/nonexistent/trekr")
            .env("CONTOUR_RQ", "/nonexistent/rq")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn contour mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Session {
            child,
            stdin,
            stdout,
            dir,
            db,
        }
    }

    /// Send a request and read its reply. Notifications use `notify`.
    fn request(&mut self, id: u32, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(id, method, params);
        let reply = self.read_line();
        assert_eq!(reply["jsonrpc"], "2.0");
        assert_eq!(reply["id"], id, "replies must match their request");
        reply
    }

    /// Write a request without waiting for it. Only the restart case needs the
    /// two halves apart, and it needs them apart for a reason: a test that
    /// blocks reading a line the server was supposed to volunteer fails by
    /// hanging, which says nothing and costs a CI slot.
    fn send(&mut self, id: u32, method: &str, params: serde_json::Value) {
        let message =
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// The next line the server writes, whatever it is.
    fn read_line(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("a line");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("line was not JSON ({e}): {line:?}"))
    }

    fn notify(&mut self, method: &str) {
        self.notify_with(method, serde_json::Value::Null);
    }

    /// A notification that carries params — `notifications/cancelled` is the
    /// only one a client sends that this server acts on.
    fn notify_with(&mut self, method: &str, params: serde_json::Value) {
        let message = serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params});
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Call a tool and parse the JSON its text content carries.
    fn tool(&mut self, id: u32, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let reply = self.request(
            id,
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        );
        assert!(
            reply["error"].is_null(),
            "{name} raised a protocol error: {reply}"
        );
        assert_ne!(
            reply["result"]["isError"], true,
            "{name} failed: {}",
            reply["result"]["content"][0]["text"]
        );
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        serde_json::from_str(text).expect("tool payload is JSON")
    }

    /// The raw payload, for the one test that cares how it is spelled.
    fn tool_text(&mut self, id: u32, name: &str, arguments: serde_json::Value) -> String {
        let reply = self.request(
            id,
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        );
        reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text content")
            .to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.db.display()));
        }
    }
}

fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "billing.rb",
            "class Invoice\n  def unpaid_for(customer)\n    where(customer: customer, paid_at: nil).order(:created_at)\n  end\n\n  def settle!(at)\n    update!(paid_at: at)\n    Notifier.deliver(self)\n  end\nend\n",
        ),
        (
            "ledger.rb",
            "class Ledger\n  def settle!(at)\n    update!(paid_at: at)\n    Notifier.deliver(self)\n  end\nend\n",
        ),
        (
            "lib.rs",
            "impl Widget {\n    fn run(&self, count: u8) -> u8 {\n        count + 1\n    }\n}\n",
        ),
    ]
}

/// The whole arc a client performs, in order, against one process.
#[test]
fn a_client_can_handshake_list_and_call() {
    let mut mcp = Session::start("arc", &corpus());

    let init = mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "contour");

    // Every client sends this, and answering it would desynchronise the
    // stream — the next reply we read must belong to the NEXT request.
    mcp.notify("notifications/initialized");

    let listed = mcp.request(2, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "search",
            "similar",
            "dupes",
            "symbols",
            "status",
            "pending",
            "store_summary",
            "index"
        ]
    );

    // `symbols` needs no index, which is what makes it usable on first
    // contact with an unknown repository.
    let outline = mcp.tool(3, "symbols", serde_json::json!({"file": "billing.rb"}));
    assert_eq!(outline["units"][0]["name"], "unpaid_for");
    assert_eq!(outline["units"][0]["lang"], "ruby");

    let indexed = mcp.tool(4, "index", serde_json::json!({}));
    assert_eq!(indexed["indexed"]["units"], 4);

    // The duplicate across two classes, found through the same lib the CLI
    // uses — and disclosing its tier.
    let dupes = mcp.tool(5, "dupes", serde_json::json!({"min_lines": 4}));
    let group = &dupes["groups"][0];
    assert_eq!(group["how"], "structural");
    let ids: Vec<&str> = group["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["Invoice#settle!", "Ledger#settle!"]);
}

/// The disclosure contract, which is the reason this surface exists: an agent
/// must be able to tell a thin answer from a complete one.
#[test]
fn tool_results_carry_the_disclosure_fields() {
    let mut mcp = Session::start("disclosure", &corpus());
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let answer = mcp.tool(3, "search", serde_json::json!({"query": "unpaid"}));
    // Nothing is summarized, so the semantic half answered from identifiers —
    // and says which tier it used, because an agent cannot see that a hit was
    // matched on a name rather than on behaviour.
    assert_eq!(answer["coverage_state"], "none");
    assert!(answer["embedder"].is_string());
    assert!(answer["withheld"].is_number());
    assert_eq!(answer["tiers"]["summary"], 0);
    assert_eq!(answer["hits"][0]["id"], "Invoice#unpaid_for");
    assert_eq!(answer["hits"][0]["semantic_via"], "identifier");
    // The path policy reaches an agent as data, not as a stderr line it never
    // sees: what class each hit is, what was withheld, and the ranking knob
    // that was applied (DEC-022).
    assert_eq!(answer["hits"][0]["class"], "app");
    assert_eq!(answer["withheld_paths"]["total"], 0);
    assert_eq!(answer["discount"], 0.5);

    let status = mcp.tool(4, "status", serde_json::json!({}));
    assert_eq!(status["checkouts"][0]["units"], 4);
    assert_eq!(
        status["checkouts"][0]["coverage"].as_array().map(Vec::len),
        Some(0),
        "no model has summarized anything yet"
    );

    // The near tier is Ruby-only, and an agent must not read its silence on a
    // Rust file as "no near duplicates here".
    let near = mcp.tool(
        5,
        "dupes",
        serde_json::json!({"min_lines": 1, "near": true}),
    );
    // The corpus holds exactly one Rust body, and the skip is attributed to
    // its language rather than to its size — two reasons an agent would act on
    // differently.
    assert_eq!(near["near_stats"]["uncovered_lang"].as_u64(), Some(1));
    assert!(
        near["withheld_paths"]["total"].is_number(),
        "a dupes result says what it withheld, in every format: {near}"
    );

    // The one-serialization rule, verified rather than assumed: canonicality
    // reaches an agent through the same JSON the CLI prints, disclosure and
    // all. `billing.rb` and `ledger.rb` hold one clone pair.
    let ranked = mcp.tool(
        6,
        "dupes",
        serde_json::json!({"min_lines": 1, "canonical": true}),
    );
    let canonical = &ranked["groups"][0]["canonical"];
    assert!(canonical["basis"].is_string(), "got {ranked}");
    let signals: Vec<&str> = canonical["signals"]
        .as_array()
        .expect("signals")
        .iter()
        .map(|s| s["signal"].as_str().unwrap())
        .collect();
    assert_eq!(signals, ["git_age", "references", "namespace_depth"]);
    // Every member was committed together, so age ties; trekr is unrunnable
    // here, so references is unavailable and says so rather than guessing.
    assert_eq!(canonical["signals"][0]["status"], "tied");
    assert_eq!(canonical["signals"][1]["status"], "unavailable");
    assert!(
        canonical["signals"][1]["note"]
            .as_str()
            .is_some_and(|n| n.contains("not installed")),
        "got {}",
        canonical["signals"][1]
    );
    assert!(ranked["canonical_stats"]["git_probes"].as_u64().unwrap() >= 2);
}

/// A bad call must not take the session down: the model reads the message and
/// tries something else.
#[test]
fn a_failing_call_leaves_the_session_usable() {
    let mut mcp = Session::start("recover", &corpus());
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );

    let bad = mcp.request(
        2,
        "tools/call",
        serde_json::json!({"name": "similar", "arguments": {"unit": "Nope#missing"}}),
    );
    assert!(
        bad["error"].is_null(),
        "a tool failure is not a protocol error"
    );
    assert_eq!(bad["result"]["isError"], true);

    // Still alive, still in sync.
    let outline = mcp.tool(3, "symbols", serde_json::json!({"file": "lib.rs"}));
    assert_eq!(outline["units"][0]["id"], "Widget::run");
}

/// Nothing but protocol may reach stdout, or the client sees a malformed
/// server. Checked by parsing every line of a whole session.
#[test]
fn stdout_carries_only_json_rpc() {
    let dir = std::env::temp_dir().join(format!("contour-mcp-{}-clean", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.rb"), "class A\n  def run; 1; end\nend\n").unwrap();

    let script = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        // Deliberately fails: contour is not a git repo here.
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"index","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"ping","params":{}}"#,
    ]
    .join("\n");

    let mut child = Command::new(env!("CARGO_BIN_EXE_contour"))
        .arg("mcp")
        .current_dir(&dir)
        .env("CONTOUR_DB", dir.join("x.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        4,
        "one reply per request, none for the notification"
    );
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("stdout line not JSON ({e}): {line:?}"));
        assert_eq!(parsed["jsonrpc"], "2.0");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[allow(dead_code)]
fn unused(_: &Path) {}

/// The contribution loop end to end: a session asks what needs summarizing,
/// hands one back, and the next search answers from it. This is the whole of
/// organic incremental indexing, exercised through the real protocol.
#[test]
fn a_session_can_summarize_what_it_reads() {
    let mut mcp = Session::start("contribute", &corpus());
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let pending = mcp.tool(3, "pending", serde_json::json!({"model": "claude-opus-5"}));
    let version = pending["prompt_version"].as_str().unwrap().to_string();
    let units = pending["units"].as_array().unwrap();
    // Three distinct Ruby bodies — `Ledger#settle!` clones `Invoice#settle!`
    // but sits under a different owner, so its context differs and it is its
    // own piece of work — plus the one Rust fn. Four, not three: `pending`
    // re-parses each body to check it still matches the index, did that as
    // Ruby whatever the language, and so silently withheld every Rust unit in
    // the corpus from every session that asked.
    assert_eq!(units.len(), 4);
    let first = &units[0];
    assert!(first["source"].as_str().unwrap().contains("def "));
    assert!(first["context"].as_str().unwrap().contains("name: "));

    let summary = serde_json::json!({
        "summary": "Returns the invoices a customer has not paid, oldest first.",
        "primary_purpose": "unpaid invoice lookup",
        "secondary_concerns": ["ordering"],
        "side_effects": [],
        "domain": "billing",
        "patterns": ["scope"]
    });
    let accepted = mcp.tool(
        4,
        "store_summary",
        serde_json::json!({
            "unit": "Invoice#unpaid_for",
            "model": "claude-opus-5",
            "prompt_version": version,
            "summary": summary
        }),
    );
    assert_eq!(accepted["id"], "Invoice#unpaid_for");
    assert_eq!(accepted["via"], "mcp", "kept apart from any bulk fill");

    // It is no longer pending, and search now answers from meaning rather than
    // from the name — a query sharing no token with the identifier.
    let after = mcp.tool(5, "pending", serde_json::json!({"model": "claude-opus-5"}));
    assert_eq!(after["units"].as_array().unwrap().len(), 3, "four less one");

    let found = mcp.tool(
        6,
        "search",
        serde_json::json!({"query": "customer has not settled their bill"}),
    );
    assert_eq!(found["tiers"]["summary"], 1, "one unit has a summary now");

    // `status` and `search` must not disagree about the same corpus. They did:
    // status counted only what an API fill had bought, so a contribution made
    // it say `none 0/128` about a corpus search was already answering from.
    // Found by QA; the numbers now come from one question each, both reported.
    let status = mcp.tool(7, "status", serde_json::json!({}));
    let answerable = &status["checkouts"][0]["answerable"];
    assert_eq!(answerable["summarized"], found["coverage"]["summarized"]);
    assert_eq!(
        answerable["summarizable"],
        found["coverage"]["summarizable"]
    );
    assert_eq!(answerable["state"], found["coverage_state"]);
    // And the contribution is visible as its own source rather than absent.
    let sources = status["checkouts"][0]["coverage"].as_array().unwrap();
    assert_eq!(sources.len(), 1, "{sources:?}");
    assert_eq!(sources[0]["via"], "mcp");
    assert_eq!(sources[0]["summarized"], 1);
    let hit = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == "Invoice#unpaid_for")
        .expect("the summarized unit is findable");
    assert_eq!(hit["semantic_via"], "summary");
}

/// Every gate on the write path, through the protocol. A contribution that is
/// plausible and wrong must be refused with a message the session can act on —
/// the store never forgets, so a coercion here is permanent.
#[test]
fn contributions_are_rejected_rather_than_repaired() {
    let mut mcp = Session::start("gates", &corpus());
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let good = serde_json::json!({
        "summary": "Does a thing.",
        "primary_purpose": "thing doing",
        "secondary_concerns": [],
        "side_effects": [],
        "domain": "billing",
        "patterns": []
    });
    let call = |mcp: &mut Session, id: u32, args: serde_json::Value| -> String {
        let reply = mcp.request(
            id,
            "tools/call",
            serde_json::json!({"name": "store_summary", "arguments": args}),
        );
        assert_eq!(reply["result"]["isError"], true, "should have been refused");
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let stale = call(
        &mut mcp,
        3,
        serde_json::json!({"unit": "Invoice#unpaid_for", "model": "m",
                           "prompt_version": "mcp-v0", "summary": good}),
    );
    assert!(stale.contains("prompt version"), "{stale}");

    let unknown = call(
        &mut mcp,
        4,
        serde_json::json!({"unit": "Nope#gone", "model": "m",
                           "prompt_version": "mcp-v1", "summary": good}),
    );
    assert!(unknown.contains("no unit named"), "{unknown}");

    let mut invented = good.clone();
    invented["side_effects"] = serde_json::json!(["teleports"]);
    let vocabulary = call(
        &mut mcp,
        5,
        serde_json::json!({"unit": "Invoice#unpaid_for", "model": "m",
                           "prompt_version": "mcp-v1", "summary": invented}),
    );
    assert!(vocabulary.contains("not a side effect"), "{vocabulary}");

    // Nothing was stored by any of the above: three Ruby bodies and one Rust
    // fn, all still waiting.
    let pending = mcp.tool(6, "pending", serde_json::json!({"model": "m"}));
    assert_eq!(pending["units"].as_array().unwrap().len(), 4);
}

/// A resident server outlives the binary it was launched from. Install a new
/// contour and the running process is instantly the old one; the moment
/// anything brings the shared database up to the new schema, every tool that
/// reads it fails for the rest of the session. Two field trials lost their
/// whole grazing budget to that, with no in-session recovery.
///
/// So the server becomes the new build. This drives the real arc: launch from a
/// copy, replace the copy underneath a live session, and check that the next
/// request is served by the replacement — same pid, same pipes, no handshake.
#[test]
fn a_server_restarts_into_a_contour_installed_underneath_it() {
    let installed = std::env::temp_dir().join(format!("contour-upgrade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&installed);
    std::fs::create_dir_all(&installed).unwrap();
    let program = installed.join("contour");
    std::fs::copy(env!("CARGO_BIN_EXE_contour"), &program).unwrap();

    let mut mcp = Session::start_using("upgrade", &corpus(), &program, &[]);
    let init = mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
    );
    // Declared because it is true: this server can restart into a build with a
    // different tool list, and the notification it then sends is only one a
    // client may act on if the handshake said so.
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], true);
    mcp.notify("notifications/initialized");
    // Healthy first, so a later failure cannot be blamed on the session never
    // having worked.
    let outline = mcp.tool(
        2,
        "symbols",
        serde_json::json!({"file": mcp.dir.join("billing.rb").to_string_lossy()}),
    );
    assert_eq!(outline["units"].as_array().map(Vec::len), Some(2));

    // Upgrade one: the same contour, freshly installed. A client holding the
    // replaced build's tool list has no way to know anything happened, so the
    // restart tells it — and answering calls correctly while describing them
    // wrongly would be a half-heal.
    install(&installed, &program, |staged| {
        std::fs::copy(env!("CARGO_BIN_EXE_contour"), staged).unwrap();
    });
    // One last answer from the build that started the session — it owes this
    // request a reply — and the restart happens after that reply goes out.
    let still_ours = mcp.tool(
        3,
        "symbols",
        serde_json::json!({"file": mcp.dir.join("ledger.rb").to_string_lossy()}),
    );
    assert_eq!(still_ours["units"].as_array().map(Vec::len), Some(1));

    // Asked before read, so a notification that never comes fails on the wrong
    // line instead of blocking on one that will never arrive.
    mcp.send(4, "ping", serde_json::json!({}));
    assert_eq!(
        mcp.read_line()["method"],
        "notifications/tools/list_changed",
        "the restarted build should tell the client to re-read its tools"
    );
    // Still serving, on the same pid and the same pipes.
    assert_eq!(mcp.read_line()["id"], 4);
    let across = mcp.tool(
        5,
        "symbols",
        serde_json::json!({"file": mcp.dir.join("billing.rb").to_string_lossy()}),
    );
    assert_eq!(across["units"].as_array().map(Vec::len), Some(2));

    // Upgrade two, so a session that outlives two installs is covered as well
    // as one. This time a stand-in rather than contour: what is left to prove
    // is that the process becomes whatever is at that path, which a copy of the
    // same code cannot show — and that the restart marker crosses the exec.
    install(&installed, &program, |staged| {
        std::fs::write(
            staged,
            "#!/bin/sh\nwhile read -r _; do\n  printf '%s\\n' \
             '{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"restarted\":\"'\"$CONTOUR_MCP_RESTARTED\"'\"}}'\ndone\n",
        )
        .unwrap();
    });
    mcp.request(6, "ping", serde_json::json!({}));
    let after = mcp.request(7, "tools/list", serde_json::json!({}));
    assert_eq!(
        after["result"]["restarted"], "1",
        "the stand-in should be answering, and told it replaced a build: {after}"
    );

    let _ = std::fs::remove_dir_all(&installed);
}

/// Put a program at `program`, the way an installer does: staged beside it and
/// **renamed** over. Truncating the running binary in place instead kills the
/// process before it can exec anything — a fact about installers rather than
/// about the server, and one a live run found by doing it the other way.
fn install(dir: &Path, program: &Path, write: impl FnOnce(&Path)) {
    use std::os::unix::fs::PermissionsExt;
    let staged = dir.join("contour.new");
    write(&staged);
    std::fs::set_permissions(&staged, PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::rename(&staged, program).unwrap();
}

/// A tool result is read by a model, so it is not pretty-printed.
///
/// Indentation was 39% of what `symbols` cost to say the same thing — measured
/// across five tools on contour's own source, 32.7k characters of payload down
/// to 28.3k, and `symbols` alone from 5236 to 3192. Together with dropping the
/// `via: null` that most units carry, an outline went from 6.9x the cost of the
/// same answer on the command line to 4.0x.
///
/// Pinned because the fix is one call and looks like a downgrade: the CLI still
/// pretty-prints, and a reader comparing the two would reasonably "fix" this.
#[test]
fn a_tool_payload_is_not_pretty_printed() {
    let mut mcp = Session::start("compact", &corpus());
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    let file = mcp.dir.join("billing.rb");
    let text = mcp.tool_text(2, "symbols", serde_json::json!({"file": file}));

    assert!(text.starts_with("{\""), "not compact: {text}");
    assert!(!text.contains("\n  "), "indented: {text}");
    // Still the same answer, and still every field it had.
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(payload["units"].as_array().unwrap().len() > 1);
    assert!(payload["units"][0]["id"].is_string());
    // An absent option is absent, not spelled out — the rule every other
    // record contour serializes already followed.
    assert!(text.contains("\"visibility\""));
    assert!(
        !text.contains("null"),
        "an empty option was serialized: {text}"
    );
}

/// `similar` takes a `scope`, and it means what `search`'s and `dupes`' means:
/// a path prefix the *answers* must lie under. The unit asked about is still
/// found wherever it lives — a scope that excludes it is a narrower search,
/// not a missing unit — and the answer says which scope it searched, because
/// three neighbours from one directory read exactly like three from a thin
/// corpus.
///
/// Field-reported: without a scope, `similar` on a monorepo has to hold a
/// vector for every unit in it.
#[test]
fn similar_takes_a_scope_and_discloses_it() {
    let body = "class %C%\n  def save(a)\n    b = a.check\n    persist(b)\n    b\n  end\nend\n";
    let mut mcp = Session::start(
        "similar-scope",
        &[
            ("a/one.rb", &body.replace("%C%", "Widget")),
            ("b/two.rb", &body.replace("%C%", "Gadget")),
        ],
    );
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let all = mcp.tool(3, "similar", serde_json::json!({"unit": "Widget#save"}));
    assert_eq!(all["neighbors"][0]["id"], "Gadget#save");
    assert!(all["scope"].is_null(), "the whole checkout names no scope");

    // The clone is in `b`, so scoping to `a` must not report it.
    let narrowed = mcp.tool(
        4,
        "similar",
        serde_json::json!({"unit": "Widget#save", "scope": "a"}),
    );
    assert_eq!(narrowed["scope"], "a");
    assert_eq!(
        narrowed["neighbors"].as_array().map(Vec::len),
        Some(0),
        "{narrowed}"
    );

    // And the unit itself lives in `a`, outside this scope, which is a
    // smaller search rather than an error.
    let elsewhere = mcp.tool(
        5,
        "similar",
        serde_json::json!({"unit": "Widget#save", "scope": "b"}),
    );
    assert_eq!(elsewhere["scope"], "b");
    assert_eq!(elsewhere["neighbors"][0]["id"], "Gadget#save");
}

/// A corpus big enough that one query is measurably slow: 150 classes of 100
/// methods, ~15k units, 3 MB. A cold `similar` over it embeds every unit and
/// takes about 1.7 s in a debug build, where `symbols` on one file is under a
/// millisecond — a margin of three orders of magnitude, which is what lets the
/// two tests below assert on order rather than on a clock.
fn heavy_corpus() -> Vec<(String, String)> {
    (0..150)
        .map(|i| {
            let body: String = (0..100)
                .map(|j| {
                    format!(
                        "  def widget_handler_{i}_{j}(alpha, beta, gamma)\n    \
                         b = alpha.one\n    c = b.two(beta)\n    d = c.three(gamma)\n    \
                         persist(d)\n  end\n"
                    )
                })
                .collect();
            (format!("f{i}.rb"), format!("class Widget{i}\n{body}end\n"))
        })
        .collect()
}

fn as_refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect()
}

/// A cheap call is answered while a heavy one is still running.
///
/// The field report: a `symbols` call — documented as needing no index at all —
/// sat with no output for fifteen minutes behind two unscoped heavy calls,
/// because the server read a line, answered it, and only then read the next.
///
/// Asserted on **order, not a clock**: the two requests go out back to back and
/// the first line the server writes must be the cheap one's. A serial loop can
/// never do that, whatever machine it runs on.
#[test]
fn a_cheap_call_does_not_wait_behind_a_heavy_one() {
    let files = heavy_corpus();
    let mut mcp = Session::start("concurrent", &as_refs(&files));
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    mcp.send(
        3,
        "tools/call",
        serde_json::json!({"name": "similar", "arguments": {"unit": "Widget0#widget_handler_0_0"}}),
    );
    mcp.send(
        4,
        "tools/call",
        serde_json::json!({"name": "symbols", "arguments": {"file": "f0.rb"}}),
    );

    let first = mcp.read_line();
    assert_eq!(first["id"], 4, "the cheap call waited: {first}");
    let second = mcp.read_line();
    assert_eq!(second["id"], 3);
}

/// `notifications/cancelled` stops the work, not just the waiting.
///
/// The field report's fourth finding: abandoning a call client-side left the
/// server burning every core until the process was killed by hand. Nothing
/// could stop it, because nothing was reading.
///
/// **The response is the proof.** Only the worker writes it, and it writes it
/// after the call returns — so an error naming the cancellation, arriving in a
/// fraction of the time the same call takes uncancelled, says the work stopped
/// rather than that the client gave up on it.
///
/// What this pins is the *outer* half: the reader saw the notification while a
/// worker was busy, and the request stopped before its expensive write. The
/// embedding pool, which is where a real corpus spends 97% of a cold call, is
/// pinned by `embed::tests::a_cancelled_embedding_stops_rather_than_finishing`
/// instead — here the corpus is small and the embedder is the hash one, so
/// embedding 15k texts is 20 ms against a second of `put_vectors`.
#[test]
fn a_cancelled_call_stops_the_work() {
    let files = heavy_corpus();
    let mut mcp = Session::start("cancelled", &as_refs(&files));
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let started = std::time::Instant::now();
    mcp.send(
        3,
        "tools/call",
        serde_json::json!({"name": "similar", "arguments": {"unit": "Widget0#widget_handler_0_0"}}),
    );
    mcp.notify_with(
        "notifications/cancelled",
        serde_json::json!({"requestId": 3, "reason": "the user pressed escape"}),
    );

    let reply = mcp.read_line();
    let cancelled = started.elapsed();
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("cancelled"), "got {text:?}");

    // The bar is the same call on the same machine, not a constant: a wall
    // clock in a test suite is a promise about somebody else's CI box. This
    // run is still cold, because a cancelled embedding stores nothing — which
    // is the second thing worth pinning, since half-written vectors would be
    // answers no embedder produced.
    let started = std::time::Instant::now();
    mcp.tool(
        4,
        "similar",
        serde_json::json!({"unit": "Widget0#widget_handler_0_0"}),
    );
    let whole = started.elapsed();
    assert!(
        cancelled * 2 < whole,
        "cancelled in {cancelled:?} against {whole:?} for the whole call"
    );

    // And the session is still usable — a cancelled request must not poison
    // the worker that served it.
    let outline = mcp.tool(5, "symbols", serde_json::json!({"file": "f0.rb"}));
    assert_eq!(outline["units"][0]["name"], "widget_handler_0_0");
}

/// The same refusal, as a tool result: an agent gets the bill rather than a
/// two-hour silence. See the CLI case for why the budget is microseconds.
#[test]
fn an_unaffordable_call_hands_back_the_bill() {
    let mut mcp = Session::start_with_env(
        "budget",
        &corpus(),
        &[("CONTOUR_EMBED_BUDGET", "0.0000000001")],
    );
    mcp.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    mcp.tool(2, "index", serde_json::json!({}));

    let reply = mcp.request(
        3,
        "tools/call",
        serde_json::json!({"name": "search", "arguments": {"query": "settle an invoice"}}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("nothing embedded yet"), "got {text:?}");
    assert!(text.contains("Narrow the scope"), "got {text:?}");
}
