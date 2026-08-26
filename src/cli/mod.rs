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
}

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

fn dupes(scope: Option<&std::path::Path>, min_lines: u32, format: Format) -> Result<i32> {
    let here = scope.unwrap_or(std::path::Path::new("."));
    let root = crate::scan::repo_root(here)?;
    // The scope the user typed, as the checkout stores paths: relative to the
    // repository root, whatever directory they ran from.
    let absolute = std::fs::canonicalize(here)?;
    let relative = absolute
        .strip_prefix(std::fs::canonicalize(&root)?)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty());

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

fn symbols(file: &std::path::Path, format: Format) -> Result<i32> {
    let src = std::fs::read(file)?;
    let blob = crate::ruby::units(&src);
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
    let report = serde_json::json!({
        "db": path.to_string_lossy(),
        "schema": store.schema_version()?,
        "checkouts": checkouts,
        // Summaries and embeddings do not exist yet; the field is here from
        // the start because DEC-009 says coverage travels with every answer,
        // and a field that appears later is a field consumers did not expect.
        "coverage": "none",
    });
    match format {
        Format::Human => {
            println!("db       {}", crate::paths::pretty(&path.to_string_lossy()));
            println!("coverage none");
            if checkouts.is_empty() {
                println!("(nothing indexed)");
            }
            for c in &checkouts {
                println!(
                    "{}  {} files, {} blobs, {} units{}",
                    crate::paths::pretty(&c.root),
                    c.files,
                    c.blobs,
                    c.units,
                    if c.stale { "  [may be stale]" } else { "" }
                );
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
