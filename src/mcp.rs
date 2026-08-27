//! MCP over stdio: the agent surface (DEC-002's "agents first", cashed in).
//!
//! Every tool here is a thin adapter over the same library the CLI consumes,
//! and returns **exactly the JSON `--json` returns**. That is the whole design:
//! one serialization, so a disclosure field cannot exist for a human and go
//! missing for an agent. An agent needs `coverage`, `how`, `confidence` and
//! `withheld` more than a person does — a person can see a thin answer looks
//! thin, and an agent will confidently build on it.
//!
//! ## Why this is hand-rolled
//!
//! trekr took `lsp-server` for LSP rather than writing a framing loop, and the
//! same weighing was run here — it just comes out the other way. `lsp-server`
//! is two dependencies (crossbeam-channel, log) and synchronous. `rmcp` at its
//! smallest useful feature set (`server`, `transport-io`) measured **81
//! transitive crates**, including a tokio runtime and schemars, for a program
//! that is otherwise entirely synchronous.
//!
//! What MCP over stdio actually needs is newline-delimited JSON-RPC 2.0 with
//! four methods. That is the code below, in about the space this comment would
//! take to justify the alternative. The tradeoff is real and worth revisiting:
//! if contour ever wants sampling, elicitation, or an HTTP transport, the
//! hand-rolled loop stops paying and `rmcp` starts.

use anyhow::Result;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Protocol revisions this server knows how to speak. The newest first: an
/// `initialize` naming any of them is answered in that revision, and anything
/// else is answered in ours so the client can decide whether to continue.
const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Serve MCP on stdin/stdout until the client closes the stream.
///
/// **Nothing but protocol may reach stdout.** Every diagnostic in this module
/// goes to stderr, because one stray line of human text corrupts the session
/// in a way that reads to the client as a malformed server.
pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle_line(&line) else {
            continue;
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

/// One request in, at most one response out. `None` for a notification, which
/// by JSON-RPC must not be answered at all.
fn handle_line(line: &str) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        // No id is recoverable from unparseable input, so this is the one
        // error that must carry a null id.
        Err(err) => {
            return Some(
                json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {err}")}
                })
                .to_string(),
            );
        }
    };
    let method = request["method"].as_str().unwrap_or_default();
    let id = request.get("id").cloned();

    // A notification has no id and takes no answer — `notifications/initialized`
    // is the one every client sends.
    let id = id?;

    let result = match method {
        "initialize" => Ok(initialize(&request["params"])),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call(&request["params"]),
        "ping" => Ok(json!({})),
        other => {
            return Some(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unknown method `{other}`")}
                })
                .to_string(),
            );
        }
    };

    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
        // A tool that failed is a *result* with `isError`, not a protocol
        // error: the model is meant to see the message and adapt, which it
        // cannot do if the failure is swallowed by the transport layer.
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "isError": true,
                "content": [{"type": "text", "text": format!("{err:#}")}]
            }
        })
        .to_string(),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or_default();
    let version = match SUPPORTED.contains(&asked) {
        true => asked,
        false => SUPPORTED[0],
    };
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "contour", "version": env!("CARGO_PKG_VERSION")},
    })
}

/// The tool surface. Descriptions are written for a model deciding whether to
/// reach for contour at all, which is the actual decision it faces.
fn tools() -> Vec<Value> {
    let path = json!({
        "type": "string",
        "description": "A path inside the repository. Defaults to the server's working directory."
    });
    // One description for one flag, so the three tools that take it cannot
    // drift into describing the same default differently.
    let include_ignored = json!({
        "type": "boolean",
        "description": "Include paths ignored by default — migrations, generated and vendored code, \
            which are frozen or regenerated rather than consolidated. Every answer reports what it \
            withheld under `withheld_paths`."
    });
    vec![
        json!({
            "name": "search",
            "description": "Find callables by what they DO, in English — \"which methods retrieve \
                unpaid invoices\". Ranks a name match and a meaning match together. The meaning \
                half only covers summarized code, so read `coverage` on every answer: `none` \
                means this was a name match only. Each hit carries the `class` of file it lives \
                in; `test` and `fixture` hits are ranked at the disclosed `discount`, because a \
                spec that shares a name with the method it tests is rarely the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "A behavioural question in plain English."},
                    "path": path,
                    "scope": {"type": "string", "description": "Repo-relative file or directory to search within."},
                    "limit": {"type": "integer", "description": "Maximum hits. Default 10."},
                    "floor": {"type": "number", "description": "Cosine floor; 0 shows everything the default withholds."},
                    "include_ignored": include_ignored
                },
                "required": ["query"]
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "similar",
            "description": "Find callables like a given one, with the tier that found each \
                disclosed: `structural` (identical normalized body), `near_structural` (mostly \
                the same shape, with a measured Jaccard), or `semantic` (a nearby summary, with \
                the cosine). Use before writing a new method to see whether one already exists.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "`Owner#method` or `Owner.method` in Ruby, `Owner::fn` in Rust. A name that means two units is refused with both locations listed; pass `path:line` to pick one."},
                    "path": path,
                    "limit": {"type": "integer", "description": "Maximum neighbours. Default 10."},
                    "include_ignored": include_ignored
                },
                "required": ["unit"]
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "dupes",
            "description": "Report callables with identical bodies, and optionally ones that are \
                nearly identical. Sees through renames, reformatting and comments, so it finds \
                copy-paste that grep cannot. Groups are ordered by `saves_nodes` — the \
                estimated AST nodes consolidating each would remove — so the first result is \
                the one most worth acting on, and the copies, body size and similarity it is \
                estimated from travel with it. With `canonical`, each group also names which member \
                is likely the original and why — and says so when the signals disagree, which \
                usually means the old one was superseded and never deleted. Ruby gets AST-grade normalization; Rust gets a \
                token-stream hash, disclosed per group as `structural` or `token_hash`. Each group \
                carries the `class` of path its copies live in and is ranked within that \
                population: app code first, then test and fixture duplication, which is real \
                maintenance signal but a poor answer to \"what should I consolidate here\". A \
                group whose copies sit under different namespaces and read an unqualified \
                constant that resolves differently in each carries a `caveat` naming those \
                constants: the bodies are identical but the consolidation may not exist, so \
                check before merging.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "scope": {"type": "string", "description": "Repo-relative file or directory."},
                    "min_lines": {"type": "integer", "description": "Ignore bodies shorter than this. Default 4."},
                    "near": {"type": "boolean", "description": "Also report nearly-identical bodies."},
                    "near_threshold": {"type": "number", "description": "Jaccard for `near`. Default 0.8."},
                    "canonical": {"type": "boolean", "description": "Name the likely-original member of each group, with the signals behind it (git age, reference counts, namespace depth) and what each measured. Costs one git blame per body and one trekr call per Ruby name, so scope it."},
                    "include_ignored": include_ignored
                }
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "symbols",
            "description": "Outline one file's callables, in source order. Parses the file live, \
                so it needs no index and works in a repository contour has never seen. Cheaper \
                than reading a long file to find out what is in it.",
            "inputSchema": {
                "type": "object",
                "properties": {"file": {"type": "string", "description": "Path to a Ruby or Rust file."}},
                "required": ["file"]
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "status",
            "description": "What the index holds, whether a checkout looks stale, and how much of \
                it is summarized. Check this first when `search` returns less than expected.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "pending",
            "description": "Units in a scope that nothing has summarized yet, each with its                 source and the structural context to summarize it against. Use this to do an                 explicit fill: read the source, write a summary in the schema the contour skill                 gives you, and hand each one back with `store_summary`. Deduplicated, so a                 method cloned ten times is offered once.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "scope": {"type": "string", "description": "Repo-relative file or directory."},
                    "model": {"type": "string", "description": "The model that will write the summaries. Required, because coverage is per model."},
                    "limit": {"type": "integer", "description": "Maximum units to return. Default 20."}
                },
                "required": ["model"]
            },
            "annotations": {"readOnlyHint": true}
        }),
        json!({
            "name": "store_summary",
            "description": "Contribute a summary you wrote for one unit, so the next session                 does not have to read it again. Validated and rejected rather than repaired: the                 payload must match the schema exactly, and the unit's body must still be the one                 the index recorded. Contributions are kept separate from any bulk fill by the                 model that wrote them.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "unit": {"type": "string", "description": "`Owner#method` / `Owner.method` / `Owner::fn`, as `pending` or `symbols` reports it."},
                    "path": {"type": "string", "description": "Repo-relative file, required only when a name is ambiguous."},
                    "root": path,
                    "model": {"type": "string", "description": "Your own model id, e.g. `claude-opus-5`."},
                    "prompt_version": {"type": "string", "description": "The version stated by the contour skill you followed."},
                    "summary": {
                        "type": "object",
                        "description": "The structured summary. Fields: summary, primary_purpose, secondary_concerns, side_effects, domain, patterns.",
                        "properties": {
                            "summary": {"type": "string"},
                            "primary_purpose": {"type": "string"},
                            "secondary_concerns": {"type": "array", "items": {"type": "string"}},
                            "side_effects": {
                                "type": "array",
                                "items": {"type": "string", "enum": ["persists", "network", "filesystem", "mutates", "observes", "raises", "spawns"]}
                            },
                            "domain": {"type": "string"},
                            "patterns": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["summary", "primary_purpose", "secondary_concerns", "side_effects", "domain", "patterns"]
                    }
                },
                "required": ["unit", "model", "prompt_version", "summary"]
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": true}
        }),
        json!({
            "name": "index",
            "description": "Scan a checkout and index every callable in it. Cheap and idempotent \
                — a reindex with no edits parses nothing. Run it when `status` says a checkout is \
                stale or absent.",
            "inputSchema": {"type": "object", "properties": {"path": path}},
            "annotations": {"readOnlyHint": false, "idempotentHint": true}
        }),
    ]
}

fn call(params: &Value) -> Result<Value> {
    let name = params["name"].as_str().unwrap_or_default();
    let args = &params["arguments"];
    let payload = match name {
        "search" => search(args)?,
        "similar" => similar(args)?,
        "dupes" => dupes(args)?,
        "symbols" => symbols(args)?,
        "status" => status()?,
        "index" => index(args)?,
        "pending" => pending(args)?,
        "store_summary" => store_summary(args)?,
        other => anyhow::bail!("unknown tool `{other}`"),
    };
    // Pretty-printed rather than compact: a model reads this, and the cost of
    // the whitespace is far below the cost of it misreading a nested field.
    Ok(json!({
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&payload)?}]
    }))
}

/// The path policy for a tool call: the checkout's, plus this call's
/// `include_ignored`. Named apart from `scoped` because a tool may want one
/// without the other.
fn classes(root: &Path, args: &Value) -> Result<crate::paths::Classes> {
    Ok(crate::paths::Classes::load(root)?
        .including_ignored(args["include_ignored"].as_bool() == Some(true)))
}

/// Resolve a tool's `path` argument the way the CLI resolves its cwd.
fn scoped(args: &Value) -> Result<(PathBuf, Option<String>)> {
    let here = args["path"].as_str().unwrap_or(".");
    let root = crate::scan::repo_root(Path::new(here))?;
    let scope = args["scope"]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty() && s != ".");
    Ok((root, scope))
}

/// [`scoped`], plus the store with this checkout brought up to date
/// (`index::open`). Every tool that answers from the index goes through here,
/// for the reason spelled out there: an agent cannot tell a confidently stale
/// answer from a correct one.
fn opened(args: &Value) -> Result<(crate::index::Opened, Option<String>)> {
    let (root, scope) = scoped(args)?;
    Ok((crate::index::open(&root)?, scope))
}

/// Attach what the refresh took to a tool result.
///
/// The CLI prints this on stderr, where all its diagnostics go in every
/// format; a tool call has no stderr, so an agent gets the same fact as a
/// field. Only when something moved — a field that is always `false` teaches a
/// model to stop reading it.
fn answered(mut result: Value, refreshed: &crate::store::Indexed) -> Value {
    if refreshed.changed {
        result["refreshed"] = json!({
            "files": refreshed.files,
            "parsed": refreshed.parsed,
        });
    }
    result
}

fn search(args: &Value) -> Result<Value> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("`query` is required"))?;
    let (opened, scope) = opened(args)?;
    let classes = classes(Path::new(&opened.root), args)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Query);
    let mut store = opened.store;
    let answer = crate::search::search(
        &mut store,
        &opened.root,
        query,
        embedder.as_ref(),
        crate::search::Options {
            scope: scope.as_deref(),
            limit: args["limit"].as_u64().unwrap_or(10) as usize,
            floor: match args["floor"].as_f64() {
                Some(floor) => floor as f32,
                None => crate::search::relevance_floor(embedder.kind()),
            },
            ..crate::search::Options::new(&classes)
        },
    )?;
    Ok(answered(serde_json::to_value(answer)?, &opened.refreshed))
}

fn similar(args: &Value) -> Result<Value> {
    let unit = args["unit"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("`unit` is required"))?;
    let (opened, _) = opened(args)?;
    let classes = classes(Path::new(&opened.root), args)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Query);
    let mut store = opened.store;
    let neighbors = crate::search::similar(
        &mut store,
        &opened.root,
        unit,
        embedder.as_ref(),
        args["limit"].as_u64().unwrap_or(10) as usize,
        &classes,
    )?;
    Ok(answered(
        serde_json::to_value(neighbors)?,
        &opened.refreshed,
    ))
}

fn dupes(args: &Value) -> Result<Value> {
    let (opened, scope) = opened(args)?;
    let classes = classes(Path::new(&opened.root), args)?;
    let (store, root) = (opened.store, opened.root);
    let min_lines = args["min_lines"].as_u64().unwrap_or(4) as u32;
    let mut found = crate::dupes::find(&store, &root, scope.as_deref(), min_lines, &classes)?;
    let mut stats = None;
    if args["near"].as_bool() == Some(true) {
        let threshold = args["near_threshold"]
            .as_f64()
            .map(|t| t as f32)
            .unwrap_or(crate::near::NEAR_THRESHOLD);
        let (near, near_stats) = crate::dupes::find_near(
            &store,
            &root,
            scope.as_deref(),
            min_lines,
            threshold,
            &classes,
        )?;
        found.groups.extend(near.groups);
        found.withheld.merge(&near.withheld);
        crate::dupes::rank(&mut found.groups);
        stats = Some(near_stats);
    }
    // Before canonicality, so a group that may not be consolidatable at all is
    // never crowned without the caveat beside it.
    let scoped = crate::constants::annotate(Path::new(&root), &mut found.groups);
    let mut ranked = None;
    if args["canonical"].as_bool() == Some(true) {
        ranked = Some(crate::canonical::annotate(
            Path::new(&root),
            &mut found.groups,
        )?);
    }
    // The scale and coverage disclosure the CLI prints to stderr has nowhere
    // to go in a tool result but the result itself — and an agent needs to
    // know the near tier skipped its Rust files, and that groups were withheld.
    Ok(answered(
        json!({
            "root": root,
            "groups": found.groups,
            "near_stats": stats,
            "canonical_stats": ranked,
            "constant_stats": scoped,
            "withheld_paths": found.withheld,
        }),
        &opened.refreshed,
    ))
}

fn symbols(args: &Value) -> Result<Value> {
    let file = args["file"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("`file` is required"))?;
    let path = Path::new(file);
    anyhow::ensure!(
        !path.is_dir(),
        "{file} is a directory; `symbols` outlines one file"
    );
    anyhow::ensure!(path.exists(), "{file} does not exist");
    let src = std::fs::read(path)?;
    let blob = crate::index::units_at(file, &src)
        .ok_or_else(|| anyhow::anyhow!("no extractor for {file}"))?;
    Ok(json!({
        "file": path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        "units": blob.units,
        "parse_errors": blob.parse_errors,
        "lines": blob.lines,
    }))
}

fn status() -> Result<Value> {
    let store = crate::store::open_default()?;
    let checkouts = store.status()?;
    let sources = store.summary_sources()?;
    let mut rows = Vec::new();
    for checkout in &checkouts {
        let mut coverage = Vec::new();
        for (model, via) in &sources {
            let counts = crate::summary::coverage(&store, &checkout.root, model, via)?;
            coverage.push(json!({
                "model": model,
                "via": via,
                "state": counts.state(),
                "summarized": counts.summarized,
                "summarizable": counts.summarizable,
            }));
        }
        let answerable = crate::summary::answerable(&store, &checkout.root)?;
        rows.push(json!({
            "root": checkout.root,
            "files": checkout.files,
            "blobs": checkout.blobs,
            "units": checkout.units,
            "stale": checkout.stale,
            "answerable": {
                "state": answerable.state(),
                "summarized": answerable.summarized,
                "summarizable": answerable.summarizable,
            },
            "coverage": coverage,
        }));
    }
    Ok(json!({
        "db": crate::store::default_path()?.to_string_lossy(),
        "checkouts": rows,
    }))
}

fn pending(args: &Value) -> Result<Value> {
    let model = args["model"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("`model` is required"))?;
    // Refreshed like any other read of the index — offering a session a unit
    // that has moved is asking it to pay for a summary the store will refuse.
    let (opened, scope) = opened(args)?;
    let units = crate::summary::pending(
        &opened.store,
        Path::new(&opened.root),
        scope.as_deref(),
        model,
        args["limit"].as_u64().unwrap_or(20) as usize,
    )?;
    Ok(answered(
        json!({
            "prompt_version": crate::summary::contributed::CONTRIBUTED_PROMPT_VERSION,
            "units": units,
        }),
        &opened.refreshed,
    ))
}

fn store_summary(args: &Value) -> Result<Value> {
    let field = |name: &str| -> Result<&str> {
        args[name]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("`{name}` is required"))
    };
    // The contribution is checked against the body the index holds, so the
    // index has to hold the body that is there now.
    let here = args["root"].as_str().unwrap_or(".");
    let mut opened = crate::index::open(Path::new(here))?;
    let accepted = crate::summary::contributed::store(
        &mut opened.store,
        Path::new(&opened.root),
        field("unit")?,
        args["path"].as_str(),
        field("model")?,
        field("prompt_version")?,
        &args["summary"],
    )?;
    Ok(answered(serde_json::to_value(accepted)?, &opened.refreshed))
}

fn index(args: &Value) -> Result<Value> {
    let here = args["path"].as_str().unwrap_or(".");
    let mut store = crate::store::open_default()?;
    let (root, counts) = crate::index::index(&mut store, Path::new(here))?;
    Ok(json!({"root": root, "indexed": counts}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(line: &str) -> Value {
        serde_json::from_str(&handle_line(line).expect("a response")).unwrap()
    }

    #[test]
    fn initialize_answers_in_a_version_the_client_named() {
        let reply = request(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize",
                "params":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        );
        assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(reply["result"]["serverInfo"]["name"], "contour");
        assert!(reply["result"]["capabilities"]["tools"].is_object());

        // An unknown revision gets ours, so the client can decide rather than
        // being told a version we cannot actually speak.
        let unknown = request(
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize",
                "params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(unknown["result"]["protocolVersion"], SUPPORTED[0]);
    }

    /// A notification has no id and must produce no output at all. Answering
    /// one is the classic way to desynchronise a client.
    #[test]
    fn a_notification_is_not_answered() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn every_tool_declares_a_usable_schema() {
        let tools = tools();
        assert_eq!(tools.len(), 8);
        for tool in &tools {
            let name = tool["name"].as_str().expect("a name");
            assert!(
                tool["description"].as_str().is_some_and(|d| d.len() > 40),
                "{name} needs a description a model can choose from"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
            // Every declared required field must exist in properties, or a
            // client generates a call that cannot validate.
            for required in tool["inputSchema"]["required"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                let key = required.as_str().unwrap();
                assert!(
                    !tool["inputSchema"]["properties"][key].is_null(),
                    "{name} requires `{key}` but does not describe it"
                );
            }
        }
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let reply = request(r#"{"jsonrpc":"2.0","id":9,"method":"tools/nope"}"#);
        assert_eq!(reply["error"]["code"], -32601);
    }

    /// A failing tool is a result the model can read and adapt to, not a
    /// transport error it never sees.
    #[test]
    fn a_failing_tool_returns_an_error_result_not_a_protocol_error() {
        let reply = request(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"symbols","arguments":{"file":"/nope/missing.rb"}}}"#,
        );
        assert!(reply["error"].is_null(), "not a protocol error");
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"].is_string());
    }

    #[test]
    fn unparseable_input_is_reported_against_a_null_id() {
        let reply = request("{not json");
        assert_eq!(reply["error"]["code"], -32700);
        assert!(reply["id"].is_null());
    }
}
