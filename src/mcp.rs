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
use std::time::SystemTime;

/// Protocol revisions this server knows how to speak. The newest first: an
/// `initialize` naming any of them is answered in that revision, and anything
/// else is answered in ours so the client can decide whether to continue.
const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Serve MCP on stdin/stdout until the client closes the stream.
///
/// **Nothing but protocol may reach stdout.** Every diagnostic in this module
/// goes to stderr, because one stray line of human text corrupts the session
/// in a way that reads to the client as a malformed server.
///
/// ## Upgrading underneath a live session
///
/// This process outlives the binary it was launched from. Install a new contour
/// and the resident server is instantly the old one — and the moment anything
/// brings the shared database up to the new schema, every tool that reads it
/// fails for the rest of the session, with no way for the session to recover.
/// Two field trials lost their whole grazing budget to exactly that.
///
/// So the server becomes the new build instead: it stamps the binary it was
/// launched from, restats it after each answer, and `exec`s the replacement in
/// place when it differs. `exec` keeps the pid and the file descriptors, so the
/// client's pipes survive and it never learns that the program on the other end
/// was replaced. Nothing needs carrying across, because [`handle_line`] holds
/// no session state — a property a test pins, since the restart depends on it.
pub fn serve() -> Result<()> {
    let launched_from = stamp();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle_line(&line, &launched_from) else {
            continue;
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
        // The one moment this process owes nobody anything: an answer has just
        // gone out and the next request has not been read. A client that
        // pipelined a batch can still have siblings sitting in a buffer we
        // cannot see, and those are lost — the known edge of doing this in
        // process, and the reason `docs/DECISIONS.md` records the proxy-parent
        // shim as the shape that removes it.
        if let Some(newer) = superseded(&launched_from) {
            let err = restart(&newer);
            eprintln!(
                "contour: could not restart into {} ({err}); still serving the \
                 build this session started with",
                newer.display()
            );
        }
    }
    Ok(())
}

/// A binary's identity on disk: where it is, how big it is, when it changed.
type Stamp = (PathBuf, u64, SystemTime);

/// What the binary this process was launched from looks like on disk.
///
/// Size and mtime rather than a hash: this is taken after every answer, and a
/// 26 MB digest per request would be a real cost to detect something an
/// installer always changes. `None` when the path cannot be read at all.
fn stamp() -> Option<Stamp> {
    let path = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    Some((path, meta.len(), meta.modified().ok()?))
}

/// The binary to become, if the one on disk is no longer the one running.
///
/// Both stamps must be readable. A path that has *become* unreadable is not an
/// upgrade — exec'ing it would fail on every request from then on — and a path
/// that was never readable cannot tell us anything changed.
fn superseded(launched_from: &Option<Stamp>) -> Option<PathBuf> {
    let (was, now) = (launched_from.as_ref()?, stamp()?);
    match *was == now {
        true => None,
        false => Some(now.0),
    }
}

/// Become the binary at `path`, keeping this process's pid and open files.
///
/// Returns only on failure — a successful `exec` never comes back. Unix only,
/// like `$HOME`-relative storage and every other assumption in this tool.
fn restart(path: &Path) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(path)
        .args(std::env::args_os().skip(1))
        .exec()
}

/// One request in, at most one response out. `None` for a notification, which
/// by JSON-RPC must not be answered at all.
///
/// Holds no state between calls, deliberately: a request is answered the same
/// way whether or not this process is the one the client handshook with, which
/// is what lets [`serve`] replace itself mid-session without replaying
/// anything.
fn handle_line(line: &str, launched_from: &Option<Stamp>) -> Option<String> {
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
        Err(err) => {
            let mut text = format!("{err:#}");
            // Almost always the schema-skew refusal, and the caller's next move
            // depends on something the message cannot know: whether a newer
            // contour is already installed. When it is, "upgrade contour" is
            // advice they have taken, and what they need to hear instead is
            // that no session restart is required.
            if superseded(launched_from).is_some() {
                text.push_str(
                    "\n\nA newer contour is installed than the one serving this session. \
                     The server is restarting into it now — retry this call; there is no \
                     need to restart the session.",
                );
            }
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"isError": true, "content": [{"type": "text", "text": text}]}
            })
            .to_string()
        }
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
                it is summarized. Check this first when `search` returns less than expected. \
                Reports every checkout on this machine, or with `path`, just the one containing \
                it.",
            "inputSchema": {"type": "object", "properties": {"path": path}},
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
        "status" => status(args)?,
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

/// A string argument a tool cannot run without, with the one wording every
/// tool reports a missing one in.
fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args[name]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("`{name}` is required"))
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
    let query = required(args, "query")?;
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
    let unit = required(args, "unit")?;
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
    let file = required(args, "file")?;
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

fn status(args: &Value) -> Result<Value> {
    let store = crate::store::open_default()?;
    let checkouts = crate::store::checkouts(&store, args["path"].as_str().map(Path::new))?;
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
    let model = required(args, "model")?;
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
    // The contribution is checked against the body the index holds, so the
    // index has to hold the body that is there now.
    let here = args["root"].as_str().unwrap_or(".");
    let mut opened = crate::index::open(Path::new(here))?;
    let accepted = crate::summary::contributed::store(
        &mut opened.store,
        Path::new(&opened.root),
        required(args, "unit")?,
        args["path"].as_str(),
        required(args, "model")?,
        required(args, "prompt_version")?,
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
        serde_json::from_str(&handle_line(line, &None).expect("a response")).unwrap()
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
        assert!(
            handle_line(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &None
            )
            .is_none()
        );
    }

    /// The property [`serve`]'s self-restart rests on: a request is answered
    /// the same way whether or not this process saw the handshake. If a tool
    /// ever starts requiring `initialize`, a session that restarts into a new
    /// build breaks on its very next call — so this is the guard, not a test of
    /// protocol leniency.
    #[test]
    fn a_tool_call_needs_no_handshake_before_it() {
        let reply = request(
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{"name":"symbols","arguments":{"file":"/nope/missing.rb"}}}"#,
        );
        assert!(reply["error"].is_null(), "answered without an initialize");
        assert_eq!(reply["result"]["isError"], true, "the file, not the state");
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

    /// The one thing a model can act on when a call is rejected is which
    /// argument it left out, so the message names it rather than reporting
    /// that something was wrong.
    #[test]
    fn a_missing_argument_is_named_in_the_error() {
        let reply = request(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call",
                "params":{"name":"search","arguments":{}}}"#,
        );
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("a message");
        assert!(text.contains("query"), "should name the argument: {text}");
    }

    #[test]
    fn unparseable_input_is_reported_against_a_null_id() {
        let reply = request("{not json");
        assert_eq!(reply["error"]["code"], -32700);
        assert!(reply["id"].is_null());
    }
}
