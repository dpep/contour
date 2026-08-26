//! The command line: one consumer of the engine, holding no logic of its own.
//!
//! **Flags do not touch the index; subcommands do.** `--symbols` reads a file
//! and parses it, `--status` reports on the database; everything that fills or
//! queries the index is a verb (`index`, and later `dupes`, `search`,
//! `summarize`, `similar`). That is the rule a reader can predict from, and
//! the reason `--symbols` works in a directory contour has never seen.
//!
//! House conventions: `-j/--json` pretty, `-J/--ndjson` compact per line,
//! stdout is data and stderr is logs, exit 0 = hit, 1 = miss, 2 = error.

use crate::core::Unit;
use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "contour",
    version,
    about = "Semantic index of source code: search, navigation, and duplicate detection over intent.",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Outline one file's callables. Parses it live — no index required.
    #[arg(long, value_name = "FILE")]
    symbols: Option<PathBuf>,

    /// What the index holds, and whether it looks stale.
    #[arg(long)]
    status: bool,

    /// Pretty JSON.
    #[arg(short = 'j', long, global = true)]
    json: bool,

    /// One compact JSON object per line.
    #[arg(short = 'J', long, global = true)]
    ndjson: bool,

    /// Print a shell completion script and exit.
    #[arg(long, value_name = "SHELL")]
    completions: Option<clap_complete::Shell>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Scan a checkout and index every callable in it.
    Index {
        /// A path inside the repository. Defaults to the working directory.
        path: Option<PathBuf>,
    },
    /// Report units whose normalized bodies are identical.
    Dupes {
        /// A file or directory to limit the report to. Defaults to the whole
        /// checkout containing the working directory.
        scope: Option<PathBuf>,
        /// Ignore bodies shorter than this, `def` through `end`.
        #[arg(long, value_name = "N", default_value_t = DEFAULT_MIN_LINES)]
        min_lines: u32,
    },
    /// Summarize callables with an LLM, up to a budget.
    Summarize {
        /// A file or directory to limit the fill to. Defaults to the whole
        /// checkout containing the working directory.
        scope: Option<PathBuf>,
        /// Stop after this many distinct answers. Clones in identical context
        /// share one, so this bounds spend, not units covered.
        #[arg(long, value_name = "N", default_value_t = DEFAULT_BUDGET)]
        budget: usize,
        /// Replay canned summaries from a JSON file instead of calling the
        /// API. Offline, free, and what the tests use.
        #[arg(long, value_name = "FILE")]
        fixtures: Option<PathBuf>,
        /// Which model to summarize with. Part of the cache key, so switching
        /// builds an adjacent summary set rather than overwriting one
        /// (DEC-006).
        #[arg(long, value_name = "MODEL", default_value = DEFAULT_MODEL)]
        model: String,
    },
    /// Find callables by what they do, in English.
    Search {
        query: String,
        /// A file or directory to search within.
        scope: Option<PathBuf>,
        #[arg(short = 'l', long, value_name = "N", default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// Cosine below which a hit is not an answer. Defaults to the
        /// embedder's calibrated floor; `0` shows everything it withheld.
        #[arg(long, value_name = "F")]
        floor: Option<f32>,
    },
    /// Score this checkout against a labeled eval set.
    Eval {
        /// Directory holding `queries.tsv` and `pairs.tsv`.
        set: PathBuf,
        #[arg(long, value_name = "N", default_value_t = DEFAULT_MIN_LINES)]
        min_lines: u32,
    },
    /// Find callables like this one, with the tier that found each disclosed.
    Similar {
        /// `Owner#method`, `Owner.method`, or a bare name at top level.
        unit: String,
        #[arg(short = 'l', long, value_name = "N", default_value_t = DEFAULT_LIMIT)]
        limit: usize,
    },
}

/// Enough to see the shape of an answer without scrolling.
pub const DEFAULT_LIMIT: usize = 10;

/// Conservative on purpose: this is the one command that spends money, and a
/// bare `contour summarize` on rails would otherwise start ~50,000 API calls.
/// Raise it deliberately.
pub const DEFAULT_BUDGET: usize = 100;

/// DEC-006 leaves the default to the eval bake-off; until that runs, the most
/// capable model is the honest starting point — a cheap model that summarizes
/// badly would poison the eval it is supposed to be judged by.
pub const DEFAULT_MODEL: &str = "claude-opus-5";

/// Below this, structural identity stops meaning duplication.
///
/// Measured on rails (54,068 units, 3,307 files). With no floor the report is
/// 1,069 groups, and 650 of them — 61% — are three-line bodies: `def
/// initialize(place); @place = place; end` and its siblings, identical because
/// there is only one way to write a one-expression method. Dropping that one
/// line class takes the report to 318 groups and 2,566 duplicated lines; the
/// cliff is what says the short bodies are a different population rather than
/// the tail of this one.
///
/// This is a usability floor, not a calibrated threshold. The real one comes
/// from the eval set (DEC-011), and `--min-lines` exists so nobody has to
/// wait for it.
pub const DEFAULT_MIN_LINES: u32 = 4;

/// How an answer is rendered. One value, so no command can render only half of
/// itself in JSON.
#[derive(Clone, Copy, PartialEq)]
enum Format {
    Human,
    Json,
    Ndjson,
}

/// Exit codes, as the house rules define them.
const HIT: i32 = 0;
const MISS: i32 = 1;

pub fn run() -> i32 {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("contour: {err:#}");
            2
        }
    }
}

fn dispatch(cli: &Cli) -> Result<i32> {
    if let Some(shell) = cli.completions {
        clap_complete::generate(
            shell,
            &mut Cli::command(),
            "contour",
            &mut std::io::stdout(),
        );
        return Ok(HIT);
    }
    let format = match (cli.json, cli.ndjson) {
        (true, true) => bail!("--json and --ndjson are two answers to one question"),
        (true, false) => Format::Json,
        (false, true) => Format::Ndjson,
        (false, false) => Format::Human,
    };

    match (&cli.command, &cli.symbols, cli.status) {
        (Some(Command::Index { path }), _, _) => index(path.as_deref(), format),
        (Some(Command::Dupes { scope, min_lines }), _, _) => {
            dupes(scope.as_deref(), *min_lines, format)
        }
        (
            Some(Command::Summarize {
                scope,
                budget,
                fixtures,
                model,
            }),
            _,
            _,
        ) => summarize(
            scope.as_deref(),
            *budget,
            fixtures.as_deref(),
            model,
            format,
        ),
        (
            Some(Command::Search {
                query,
                scope,
                limit,
                floor,
            }),
            _,
            _,
        ) => search(query, scope.as_deref(), *limit, *floor, format),
        (Some(Command::Similar { unit, limit }), _, _) => similar(unit, *limit, format),
        (Some(Command::Eval { set, min_lines }), _, _) => eval(set, *min_lines, format),
        (None, Some(file), _) => symbols(file, format),
        (None, None, true) => status(format),
        (None, None, false) => {
            Cli::command().print_help()?;
            Ok(MISS)
        }
    }
}

fn index(path: Option<&std::path::Path>, format: Format) -> Result<i32> {
    let path = path.unwrap_or(std::path::Path::new(".")).to_path_buf();
    let mut store = crate::store::open_default()?;
    let (root, counts) = crate::index::index(&mut store, &path)?;

    match format {
        Format::Human => println!(
            "{}: {} files, {} blobs ({} parsed), {} units",
            crate::paths::pretty(&root),
            counts.files,
            counts.blobs,
            counts.parsed,
            counts.units
        ),
        _ => emit(
            format,
            &serde_json::json!({ "root": root, "indexed": counts }),
        )?,
    }
    Ok(HIT)
}

/// A path the user typed, resolved into the checkout root plus the
/// checkout-relative prefix the store keys paths by.
///
/// Shared by every command that takes a SCOPE, so "app/models" means the same
/// thing whichever one you hand it to.
fn scoped(scope: Option<&std::path::Path>) -> Result<(PathBuf, Option<String>)> {
    let here = scope.unwrap_or(std::path::Path::new("."));
    let root = crate::scan::repo_root(here)?;
    let relative = std::fs::canonicalize(here)?
        .strip_prefix(std::fs::canonicalize(&root)?)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty());
    Ok((root, relative))
}

fn dupes(scope: Option<&std::path::Path>, min_lines: u32, format: Format) -> Result<i32> {
    let (root, relative) = scoped(scope)?;
    let store = crate::store::open_default()?;
    let root = root.to_string_lossy().into_owned();
    let groups = crate::dupes::find(&store, &root, relative.as_deref(), min_lines)?;

    match format {
        Format::Human => {
            for group in &groups {
                println!(
                    "{} × {} lines  [{}]",
                    group.members.len(),
                    group.lines,
                    group.how
                );
                for member in &group.members {
                    println!(
                        "  {}:{}-{}  {}",
                        member.path, member.line, member.end_line, member.id
                    );
                }
            }
            if groups.is_empty() {
                eprintln!(
                    "contour: no clones of {min_lines}+ lines in {}",
                    crate::paths::pretty(&root)
                );
            }
        }
        _ => emit(format, &groups)?,
    }
    Ok(if groups.is_empty() { MISS } else { HIT })
}

fn summarize(
    scope: Option<&std::path::Path>,
    budget: usize,
    fixtures: Option<&std::path::Path>,
    model: &str,
    format: Format,
) -> Result<i32> {
    let (root, relative) = scoped(scope)?;
    let summarizer: Box<dyn crate::summary::Summarizer> = match fixtures {
        Some(path) => Box::new(crate::summary::fixture::Fixtures::load(path)?),
        None => Box::new(crate::summary::anthropic::Anthropic::from_env(model)?),
    };
    let mut store = crate::store::open_default()?;
    let filled = crate::summary::fill(
        &mut store,
        &root,
        relative.as_deref(),
        summarizer.as_ref(),
        budget,
    )?;

    match format {
        Format::Human => {
            println!(
                "{} summarized, {} shared, {} failed, {} left",
                filled.summarized, filled.shared, filled.failed, filled.remaining
            );
            // Tokens, not dollars: per-token rates change, and a hardcoded
            // price would quietly go wrong.
            if filled.usage.input_tokens > 0 {
                println!(
                    "tokens   {} in, {} out",
                    filled.usage.input_tokens, filled.usage.output_tokens
                );
            }
        }
        _ => emit(format, &filled)?,
    }
    // Nothing bought and nothing left to buy is a miss: the scope was already
    // complete, or held nothing summarizable.
    Ok(match filled.summarized > 0 || filled.remaining > 0 {
        true => HIT,
        false => MISS,
    })
}

fn search(
    query: &str,
    scope: Option<&std::path::Path>,
    limit: usize,
    floor: Option<f32>,
    format: Format,
) -> Result<i32> {
    let (root, relative) = scoped(scope)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Query);
    let mut store = crate::store::open_default()?;
    let answer = crate::search::search(
        &mut store,
        &root.to_string_lossy(),
        relative.as_deref(),
        query,
        embedder.as_ref(),
        limit,
        floor.unwrap_or_else(|| crate::search::relevance_floor(embedder.kind())),
    )?;

    match format {
        Format::Human => {
            for hit in &answer.hits {
                let cosine = match hit.cosine {
                    // Two decimals: a cosine off a 256-dim vector has about
                    // that much meaning, and more digits invent precision.
                    Some(c) => format!("  cos {c:.2}"),
                    None => String::new(),
                };
                println!(
                    "{}:{}  {}  [{}]{cosine}",
                    hit.path, hit.line, hit.id, hit.how
                );
                if let Some(summary) = &hit.summary {
                    println!("    {summary}");
                }
            }
            // Coverage travels with every answer (DEC-009): a search over a
            // half-summarized repo must not look like a search over a small one.
            disclose(&answer);
        }
        _ => emit(format, &answer)?,
    }
    Ok(if answer.hits.is_empty() { MISS } else { HIT })
}

/// The line that stops a thin answer from reading as a complete one.
fn disclose(answer: &crate::search::Answer) {
    let note = match answer.coverage_state {
        "complete" => String::new(),
        "none" => " — nothing is summarized yet, so this is a name match only".into(),
        _ => format!(
            " — {}/{} summarized, so the meaning half saw part of the corpus",
            answer.coverage.summarized, answer.coverage.summarizable
        ),
    };
    eprintln!(
        "contour: coverage {} via the {} embedder{note}",
        answer.coverage_state, answer.embedder
    );
    // The floor is inherited from another corpus, so what it hides has to be
    // visible or it is an unfalsifiable constant.
    if answer.withheld > 0 {
        eprintln!(
            "contour: {} result(s) below the relevance floor ({:.2}) withheld; \
             --floor 0 to see them",
            answer.withheld, answer.floor
        );
    }
}

fn similar(unit: &str, limit: usize, format: Format) -> Result<i32> {
    let (root, _) = scoped(None)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Query);
    let mut store = crate::store::open_default()?;
    let neighbors = crate::search::similar(
        &mut store,
        &root.to_string_lossy(),
        unit,
        embedder.as_ref(),
        limit,
    )?;

    match format {
        Format::Human => {
            for n in &neighbors {
                // An exact structural clone is a predicate, so it shows its
                // evidence (the body size) rather than a manufactured
                // confidence; a semantic neighbour shows the cosine, which is
                // a real graded measurement (DEC-010).
                let evidence = match (n.confidence, n.lines) {
                    (Some(c), _) => format!("  cos {c:.2}"),
                    (_, Some(lines)) => format!("  {lines} lines"),
                    _ => String::new(),
                };
                println!("{}:{}  {}  [{}]{evidence}", n.path, n.line, n.id, n.how);
                if let Some(summary) = &n.summary {
                    println!("    {summary}");
                }
            }
        }
        _ => emit(format, &neighbors)?,
    }
    Ok(if neighbors.is_empty() { MISS } else { HIT })
}

fn eval(set: &std::path::Path, min_lines: u32, format: Format) -> Result<i32> {
    let (root, _) = scoped(None)?;
    let labels = crate::eval::load(set)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Bulk);
    let mut store = crate::store::open_default()?;
    let report = crate::eval::run(&mut store, &root, &labels, embedder.as_ref(), min_lines)?;

    match format {
        Format::Human => crate::eval::render(&report),
        _ => emit(format, &report)?,
    }
    // A miss means the labeled set found nothing to score, which is a broken
    // set rather than a bad result.
    Ok(match report.rankings.first().is_some_and(|r| r.total > 0) {
        true => HIT,
        false => MISS,
    })
}

fn symbols(file: &std::path::Path, format: Format) -> Result<i32> {
    let src = std::fs::read(file)?;
    let path = file.to_string_lossy();
    let Some(lang) = crate::scan::language(&path) else {
        anyhow::bail!("no extractor for {}", file.display());
    };
    let blob = match lang {
        crate::core::Lang::Ruby => crate::ruby::units(&src),
        crate::core::Lang::Rust => crate::rust::units(&src),
    };
    if blob.parse_errors > 0 {
        // The outline is what survived, not what is there. Say so on stderr so
        // a machine consumer's stdout stays clean.
        eprintln!(
            "contour: {} parse error(s) in {}; outline is partial",
            blob.parse_errors,
            file.display()
        );
    }
    match format {
        Format::Human => {
            for unit in &blob.units {
                println!("{:>5}  {}{}", unit.line, unit.id(), signature(unit));
            }
        }
        _ => emit(format, &blob.units)?,
    }
    Ok(if blob.units.is_empty() { MISS } else { HIT })
}

fn status(format: Format) -> Result<i32> {
    let path = crate::store::default_path()?;
    let store = crate::store::open_default()?;
    let checkouts = store.status()?;
    // Coverage is reported per model that has bought anything, not for a
    // presumed one: DEC-005 lets indexes from different models coexist, so
    // "how covered am I" has no single answer.
    let models = store.summary_models()?;

    let mut rows = Vec::new();
    for checkout in &checkouts {
        let mut coverage = Vec::new();
        for model in &models {
            let counts = crate::summary::coverage(&store, &checkout.root, model)?;
            coverage.push(serde_json::json!({
                "model": model,
                "state": counts.state(),
                "summarized": counts.summarized,
                "summarizable": counts.summarizable,
            }));
        }
        rows.push(serde_json::json!({
            "root": checkout.root,
            "indexed_at": checkout.indexed_at,
            "files": checkout.files,
            "blobs": checkout.blobs,
            "units": checkout.units,
            "stale": checkout.stale,
            "coverage": coverage,
        }));
    }
    let report = serde_json::json!({
        "db": path.to_string_lossy(),
        "schema": store.schema_version()?,
        "checkouts": rows,
    });

    match format {
        Format::Human => {
            println!("db  {}", crate::paths::pretty(&path.to_string_lossy()));
            if checkouts.is_empty() {
                println!("(nothing indexed)");
            }
            for (checkout, row) in checkouts.iter().zip(&rows) {
                println!(
                    "{}  {} files, {} blobs, {} units{}",
                    crate::paths::pretty(&checkout.root),
                    checkout.files,
                    checkout.blobs,
                    checkout.units,
                    if checkout.stale {
                        "  [may be stale]"
                    } else {
                        ""
                    }
                );
                let coverage = row["coverage"].as_array().expect("built above");
                if coverage.is_empty() {
                    println!("    coverage none");
                }
                for entry in coverage {
                    // The fraction travels with the label: "warming" alone
                    // cannot tell a reader whether it means 2% or 98%.
                    println!(
                        "    coverage {} — {}/{} summarized by {}",
                        entry["state"].as_str().unwrap_or("?"),
                        entry["summarized"],
                        entry["summarizable"],
                        entry["model"].as_str().unwrap_or("?"),
                    );
                }
            }
        }
        _ => emit(format, &report)?,
    }
    Ok(if checkouts.is_empty() { MISS } else { HIT })
}

/// `(force:, dry_run: nil)` — the shape a caller sees, not the full parameter
/// record. Human output only; JSON keeps the typed list.
fn signature(unit: &Unit) -> String {
    use crate::core::ParamKind::*;
    if unit.params.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = unit
        .params
        .iter()
        .map(|p| match p.kind {
            Req | Post => p.name.clone(),
            Opt => format!("{} = …", p.name),
            Rest => format!("*{}", p.name),
            Keyreq => format!("{}:", p.name),
            Key => format!("{}: …", p.name),
            Keyrest => format!("**{}", p.name),
            Block => format!("&{}", p.name),
            Nokey => "**nil".to_string(),
        })
        .collect();
    format!("({})", parts.join(", "))
}

fn emit<T: serde::Serialize>(format: Format, value: &T) -> Result<()> {
    let out = std::io::stdout();
    let mut out = out.lock();
    match format {
        Format::Ndjson => match serde_json::to_value(value)? {
            serde_json::Value::Array(rows) => {
                for row in rows {
                    writeln!(out, "{}", serde_json::to_string(&row)?)?;
                }
            }
            other => writeln!(out, "{}", serde_json::to_string(&other)?)?,
        },
        _ => writeln!(out, "{}", serde_json::to_string_pretty(value)?)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }
}
