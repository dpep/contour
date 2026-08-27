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
    // Negative numbers reach the value parser instead of being read as flags,
    // so `--near-threshold -1` is answered by the rule it broke rather than by
    // clap's tip about quoting it.
    #[command(allow_negative_numbers = true)]
    Dupes {
        /// A file or directory to limit the report to. Defaults to the whole
        /// checkout containing the working directory.
        scope: Option<PathBuf>,
        /// Ignore bodies shorter than this, `def` through `end`.
        #[arg(long, value_name = "N", default_value_t = DEFAULT_MIN_LINES)]
        min_lines: u32,
        /// Also report bodies that are nearly the same shape, with the
        /// measured similarity on each.
        #[arg(long)]
        near: bool,
        /// Jaccard at or above which two bodies count as near-structural.
        #[arg(
            long,
            value_name = "J",
            default_value_t = crate::near::NEAR_THRESHOLD,
            value_parser = fraction
        )]
        near_threshold: f32,
        /// Name the likely-original member of each group, with the basis.
        ///
        /// Off by default because it is the only part of this report that
        /// leaves the process: one `git blame` per body and one `trekr --refs`
        /// per Ruby name. On a scope that is milliseconds; on all of rails it
        /// is minutes, and the run prints what it spent.
        #[arg(long)]
        canonical: bool,
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
    #[command(allow_negative_numbers = true)]
    Search {
        query: String,
        /// A file or directory to search within.
        scope: Option<PathBuf>,
        #[arg(short = 'l', long, value_name = "N", default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// Cosine below which a hit is not an answer. Defaults to the
        /// embedder's calibrated floor; `0` shows everything it withheld.
        #[arg(long, value_name = "F", value_parser = fraction)]
        floor: Option<f32>,
    },
    /// Serve the Model Context Protocol on stdin/stdout, for an agent client.
    Mcp,
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
        /// A path inside the repository. Defaults to the working directory.
        ///
        /// Locates the checkout only — neighbours are always sought across the
        /// whole of it, since the useful answer to "has this been written
        /// before" is rarely in the directory you are standing in.
        path: Option<PathBuf>,
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

/// A Jaccard and a cosine are both fractions of one, and a value outside that
/// range is a mistake the tool can see. `--near-threshold 1.5` used to be
/// accepted in silence and then match nothing, which reads as "no duplicates".
///
/// Written out rather than using clap's range parser because that one rejects
/// a negative by complaining about the *flag*, and the tip it prints sends the
/// reader looking for a value it has already got.
fn fraction(value: &str) -> Result<f32, String> {
    let parsed: f32 = value
        .parse()
        .map_err(|_| format!("`{value}` is not a number"))?;
    match (0.0..=1.0).contains(&parsed) {
        true => Ok(parsed),
        false => Err(format!("`{value}` is not between 0 and 1")),
    }
}

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
        (
            Some(Command::Dupes {
                scope,
                min_lines,
                near,
                near_threshold,
                canonical,
            }),
            _,
            _,
        ) => dupes(
            scope.as_deref(),
            *min_lines,
            *near,
            *near_threshold,
            *canonical,
            format,
        ),
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
        (Some(Command::Similar { unit, path, limit }), _, _) => {
            similar(unit, path.as_deref(), *limit, format)
        }
        (Some(Command::Eval { set, min_lines }), _, _) => eval(set, *min_lines, format),
        // Not a flag, per DEC-015: it serves the index.
        (Some(Command::Mcp), _, _) => {
            crate::mcp::serve()?;
            Ok(HIT)
        }
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
    let (root, counts) = crate::index::index(&mut store, &path).map_err(known_checkouts)?;

    match format {
        Format::Human => {
            // `0 parsed, 0 units` is the whole point of blob keying and reads
            // like an empty index, so the line says which it is.
            let work = match counts.parsed {
                0 => "nothing new to read".to_string(),
                parsed => format!("{parsed} blob(s) read, {} unit(s)", counts.units),
            };
            println!(
                "{}: {} files, {} blobs — {work}",
                crate::paths::pretty(&root),
                counts.files,
                counts.blobs,
            )
        }
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
    let root = crate::scan::repo_root(here).map_err(known_checkouts)?;
    let relative = std::fs::canonicalize(here)?
        .strip_prefix(std::fs::canonicalize(&root)?)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty());
    Ok((root, relative))
}

/// Add "and here is what contour does know about" to a failure to find a
/// checkout, which is almost always someone standing in the wrong directory.
///
/// Best-effort: if the database cannot be read the original error stands
/// alone, because an error message is not worth a second error.
fn known_checkouts(err: anyhow::Error) -> anyhow::Error {
    let Ok(store) = crate::store::open_default() else {
        return err;
    };
    let Ok(checkouts) = store.status() else {
        return err;
    };
    if checkouts.is_empty() {
        return err;
    }
    let listed: Vec<String> = checkouts
        .iter()
        .map(|c| format!("  {}", crate::paths::pretty(&c.root)))
        .collect();
    anyhow::anyhow!("{err}\ncontour has indexed:\n{}", listed.join("\n"))
}

fn dupes(
    scope: Option<&std::path::Path>,
    min_lines: u32,
    near: bool,
    near_threshold: f32,
    canonical: bool,
    format: Format,
) -> Result<i32> {
    let (root, relative) = scoped(scope)?;
    let store = crate::store::open_default()?;
    let root = root.to_string_lossy().into_owned();
    let mut groups = crate::dupes::find(&store, &root, relative.as_deref(), min_lines)?;
    let mut stats = None;
    if near {
        let (near_groups, found) = crate::dupes::find_near(
            &store,
            &root,
            relative.as_deref(),
            min_lines,
            near_threshold,
        )?;
        groups.extend(near_groups);
        // One order over both tiers, so a big near-duplicate is not buried
        // under every exact one regardless of what consolidating it would buy.
        crate::dupes::rank(&mut groups);
        stats = Some(found);
    }
    let ranked = match canonical {
        true => Some(crate::canonical::annotate(
            std::path::Path::new(&root),
            &mut groups,
        )?),
        false => None,
    };

    let whole = serde_json::json!({
        "root": root,
        "groups": groups,
        "near_stats": stats,
        "canonical_stats": ranked,
    });
    match format {
        Format::Human => {
            for group in &groups {
                // The estimate leads, because it is the order — and every
                // component it is built from follows, so the order can be
                // argued with rather than trusted (DEC-010). The `~` is not
                // decoration: a near consolidation leaves two thin callers
                // behind, so this is an upper bound.
                let discount = match group.similarity {
                    Some(jaccard) => format!(" × {jaccard:.2} jaccard"),
                    None => String::new(),
                };
                let size = match group.nodes {
                    Some(nodes) => format!("{} lines, {nodes} nodes", group.lines),
                    None => format!("{} lines", group.lines),
                };
                println!(
                    "~{} nodes  ·  {} × ({size}){discount}  [{}]",
                    group.saves_nodes,
                    group.members.len(),
                    group.how
                );
                let pick = group
                    .canonical
                    .as_ref()
                    .and_then(|c| c.pick.as_ref())
                    .map(|p| (p.path.as_str(), p.line));
                for member in &group.members {
                    // The winner is marked in place rather than only named
                    // below. rq holds five `tests::find`, one per language
                    // plugin, so a line naming the pick by id there says
                    // nothing a reader can act on.
                    let mark = match pick == Some((member.path.as_str(), member.line)) {
                        true => '*',
                        false => ' ',
                    };
                    println!(
                        "{mark} {}:{}-{}  {}",
                        crate::paths::within(&root, &member.path),
                        member.line,
                        member.end_line,
                        member.id
                    );
                }
                // Never a bare crown: the pick and the basis travel together,
                // and an abstention prints its reasoning too, because "the
                // signals disagree" is a finding a reader should act on.
                if let Some(canonical) = &group.canonical {
                    match &canonical.pick {
                        Some(_) => println!("  * likely canonical — {}", canonical.basis),
                        None => println!("  no canonical pick — {}", canonical.basis),
                    }
                }
            }
            if groups.is_empty() {
                eprintln!(
                    "contour: no clones of {min_lines}+ lines in {}",
                    crate::paths::pretty(&root)
                );
            }
        }
        _ => answer(format, &whole, &groups)?,
    }
    // Diagnostics, so stderr in every format — stdout stays the groups, and
    // `-J` stays one result per line. The scale claim is stated rather than
    // asserted: pairs actually compared against pairs a full scan would need.
    if let Some(ranked) = &ranked {
        eprintln!(
            "contour: canonicality took {} git blame(s) and {} trekr call(s) in {:.1}s",
            ranked.git_probes,
            ranked.trekr_probes,
            ranked.millis as f64 / 1000.0
        );
    }
    if let Some(stats) = &stats {
        eprintln!(
            "contour: near tier compared {} candidate pair(s) of {} possible \
             ({} bodies, {} sub-shapes, {} too common to use)",
            stats.candidates, stats.exhaustive, stats.bodies, stats.subtrees, stats.dropped_common
        );
        if stats.uncovered_lang > 0 {
            eprintln!(
                "contour: {} body/bodies skipped — the near tier is Ruby-only, \
                 because a Rust token hash has no sub-shapes to compare (DEC-012)",
                stats.uncovered_lang
            );
        }
        if stats.uncovered_small > 0 {
            eprintln!(
                "contour: {} body/bodies skipped — too small to hold a sub-shape \
                 worth comparing, so the near tier has no evidence about them",
                stats.uncovered_small
            );
        }
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
    // An empty query matched everything weakly and reported it as a result.
    // There is no ranking of a corpus against nothing.
    if query.trim().is_empty() {
        bail!("a search needs something to search for");
    }
    let (root, relative) = scoped(scope)?;
    let embedder = crate::embed::default_embedder(None, crate::embed::Workload::Query);
    let mut store = crate::store::open_default()?;
    let answer = crate::search::search(
        &mut store,
        &root.to_string_lossy(),
        query,
        embedder.as_ref(),
        crate::search::Options {
            scope: relative.as_deref(),
            limit,
            floor: floor.unwrap_or_else(|| crate::search::relevance_floor(embedder.kind())),
            ..Default::default()
        },
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
                    crate::paths::within(&answer.root, &hit.path),
                    hit.line,
                    hit.id,
                    hit.how
                );
                if let Some(summary) = &hit.summary {
                    println!("    {summary}");
                }
            }
            // Coverage travels with every answer (DEC-009): a search over a
            // half-summarized repo must not look like a search over a small one.
            disclose(&answer);
        }
        _ => self::answer(format, &answer, &answer.hits)?,
    }
    Ok(if answer.hits.is_empty() { MISS } else { HIT })
}

/// The line that stops a thin answer from reading as a complete one.
fn disclose(answer: &crate::search::Answer) {
    // Precise about which tier answered. "No summaries" no longer means "names
    // only" — the identifier tier embeds what code is *called*, which is a
    // real semantic answer with a real limitation, and saying otherwise would
    // undersell one and oversell the other.
    let note = match answer.coverage_state {
        "complete" => String::new(),
        "none" => format!(
            " — nothing is summarized, so {} unit(s) were matched on what they are \
             called rather than what they do",
            answer.tiers.identifier
        ),
        _ => format!(
            " — {}/{} summarized; the rest were matched on their names",
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

fn similar(
    unit: &str,
    path: Option<&std::path::Path>,
    limit: usize,
    format: Format,
) -> Result<i32> {
    let (root, _) = scoped(path)?;
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
            for n in &neighbors.neighbors {
                // An exact structural clone is a predicate, so it shows its
                // evidence (the body size) rather than a manufactured
                // confidence; a semantic neighbour shows the cosine and a near
                // one its Jaccard, because those judgments really are graded
                // (DEC-010).
                let evidence = match (n.cosine, n.similarity, n.lines) {
                    (Some(cosine), _, _) => format!("  cos {cosine:.2}"),
                    (_, Some(jaccard), _) => format!("  jaccard {jaccard:.2}"),
                    (_, _, Some(lines)) => format!("  {lines} lines"),
                    _ => String::new(),
                };
                println!(
                    "{}:{}  {}  [{}]{evidence}",
                    crate::paths::within(&neighbors.root, &n.path),
                    n.line,
                    n.id,
                    n.how
                );
                if let Some(summary) = &n.summary {
                    println!("    {summary}");
                }
            }
            // `similar` disclosed nothing at all until now, so an empty answer
            // was zero bytes and an exit code — indistinguishable from a thin
            // corpus, a cold index, or a floor doing its job.
            if neighbors.neighbors.is_empty() {
                eprintln!(
                    "contour: nothing similar to {} in {}",
                    neighbors.unit,
                    crate::paths::pretty(&neighbors.root)
                );
            }
            eprintln!(
                "contour: coverage {} via the {} embedder — {}/{} summarized",
                neighbors.coverage_state,
                neighbors.embedder,
                neighbors.coverage.summarized,
                neighbors.coverage.summarizable
            );
            if neighbors.withheld > 0 {
                eprintln!(
                    "contour: {} semantic neighbour(s) below the relevance floor \
                     ({:.2}) withheld",
                    neighbors.withheld, neighbors.floor
                );
            }
        }
        _ => answer(format, &neighbors, &neighbors.neighbors)?,
    }
    Ok(if neighbors.neighbors.is_empty() {
        MISS
    } else {
        HIT
    })
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
    // Named before it is opened. `--symbols` takes a path a person typed, so
    // its two likely mistakes — a typo and a directory — deserve their own
    // sentences rather than an `os error 21`.
    if file.is_dir() {
        bail!(
            "{} is a directory; --symbols outlines one file",
            file.display()
        );
    }
    if !file.exists() {
        bail!("{} does not exist", file.display());
    }
    let src = std::fs::read(file)
        .map_err(|err| anyhow::anyhow!("cannot read {}: {err}", file.display()))?;
    let path = file.to_string_lossy();
    let Some(blob) = crate::index::units_at(&path, &src) else {
        anyhow::bail!("no extractor for {}", file.display());
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
            // Zero bytes and exit 1 is a correct answer nobody can read.
            if blob.units.is_empty() {
                eprintln!(
                    "contour: no callables in {} ({} line(s))",
                    file.display(),
                    blob.lines
                );
            }
        }
        _ => answer(
            format,
            &serde_json::json!({
                "file": file.canonicalize().unwrap_or_else(|_| file.to_path_buf()),
                "units": blob.units,
                "parse_errors": blob.parse_errors,
                "lines": blob.lines,
            }),
            &blob.units,
        )?,
    }
    Ok(if blob.units.is_empty() { MISS } else { HIT })
}

fn status(format: Format) -> Result<i32> {
    let path = crate::store::default_path()?;
    let store = crate::store::open_default()?;
    let checkouts = store.status()?;
    // Per `(model, via)`, not per model: DEC-005 lets indexes from different
    // models coexist and DEC-018 keeps contributions in their own keyspace.
    let sources = store.summary_sources()?;

    let mut rows = Vec::new();
    for checkout in &checkouts {
        let mut coverage = Vec::new();
        for (model, via) in &sources {
            let counts = crate::summary::coverage(&store, &checkout.root, model, via)?;
            coverage.push(serde_json::json!({
                "model": model,
                "via": via,
                "state": counts.state(),
                "summarized": counts.summarized,
                "summarizable": counts.summarizable,
            }));
        }
        // What a query can actually answer from, across every source. This is
        // the number `search` discloses, and it belongs here beside the
        // per-source breakdown rather than being derivable from neither.
        let answerable = crate::summary::answerable(&store, &checkout.root)?;
        rows.push(serde_json::json!({
            "root": checkout.root,
            "indexed_at": checkout.indexed_at,
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
                // The fraction travels with the label: "warming" alone cannot
                // tell a reader whether it means 2% or 98%.
                println!(
                    "    coverage {} — {}/{} answerable",
                    row["answerable"]["state"].as_str().unwrap_or("?"),
                    row["answerable"]["summarized"],
                    row["answerable"]["summarizable"],
                );
                for entry in row["coverage"].as_array().expect("built above") {
                    println!(
                        "      {}/{} by {} via {}",
                        entry["summarized"],
                        entry["summarizable"],
                        entry["model"].as_str().unwrap_or("?"),
                        entry["via"].as_str().unwrap_or("?"),
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

/// One answer, rendered for whichever audience asked.
///
/// `--json` is the **whole** answer — records plus the disclosure that says
/// what the search could and could not see — and is byte-for-byte what the
/// MCP tool returns for the same question. That equality is the point: no
/// field may exist for an agent and go missing for a human, or the reverse.
///
/// `-J` is the records alone, one compact object per line, because that is
/// what a shell pipeline consumes. Nothing is lost: the disclosure has already
/// gone to stderr, which is where diagnostics go in every format.
fn answer<W: serde::Serialize, R: serde::Serialize>(
    format: Format,
    whole: &W,
    records: &[R],
) -> Result<()> {
    match format {
        Format::Ndjson => emit(format, &records),
        _ => emit(format, whole),
    }
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
