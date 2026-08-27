//! The labeled eval set (DEC-011), and the numbers that settle thresholds.
//!
//! Every threshold in contour is currently either inherited from another
//! corpus (the relevance floor, measured by gqls on GraphQL records) or a
//! usability guess (`--min-lines`). This module is how they stop being that.
//!
//! Three questions, three sections of the report:
//!
//! 1. **Does search work?** Rank of the expected unit for each labeled query —
//!    top-1, top-5, miss.
//! 2. **Does it beat the baseline?** The same queries through token search
//!    over names, and over source. Phase 1's exit criterion is a number, not a
//!    vibe — and the name baseline is the sharp one, because it isolates
//!    whether the expensive summary layer earns itself over free fuzzy
//!    matching.
//! 3. **Where should the thresholds sit?** The cosine of labeled answers
//!    against the best distractor per query, swept across candidate floors;
//!    and the body sizes of true against false duplicate pairs. This is gqls's
//!    floor experiment rerun on method summaries, which is what DEC-011 asks
//!    for.

use crate::embed::Embedder;
use crate::store::{Located, Store};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

/// One "this query should find this method" label.
#[derive(Debug, Clone)]
pub struct QueryLabel {
    pub query: String,
    pub expected: String,
    /// The labeller was unsure. Counted separately so a shaky label cannot
    /// quietly prop up a headline number.
    pub provisional: bool,
}

/// One "these two are (not) the same behaviour" label.
#[derive(Debug, Clone)]
pub struct PairLabel {
    pub a: String,
    pub b: String,
    pub verdict: Verdict,
    pub provisional: bool,
}

/// What a labeled pair should be found by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The exact tier must group it.
    Duplicate,
    /// The near tier must find it, and the exact tier must not.
    Near,
    /// Neither tier may report it. These are the sharp labels: each is a pair
    /// a looser normalization or a lower threshold would wrongly merge.
    Distinct,
}

/// One "of these two, this is the implementation the other shadows" label.
///
/// An *edge*, not a group: a canonical with two alternates is two rows. That
/// is the labeller's shape, and it is the right one — a shim that delegates to
/// what it shadows has a different body, so no duplicate group holds the pair,
/// and the pair is still the one most worth ranking.
#[derive(Debug, Clone)]
pub struct CanonicalLabel {
    pub canonical: String,
    pub alternate: String,
    /// Why the labeller is sure, from a closed vocabulary. Recorded and
    /// validated, but **not scored against** — a signal's job is to be right,
    /// not to be right for the reason a human wrote down.
    pub evidence: Vec<String>,
    pub provisional: bool,
}

/// The vocabulary `canonical.tsv` documents. Closed on purpose: a typo that
/// parses is a label that quietly means something else.
const EVIDENCE: [&str; 4] = ["older", "delegates", "mirrors", "ex_subclass"];

#[derive(Debug, Default)]
pub struct Labels {
    pub queries: Vec<QueryLabel>,
    pub pairs: Vec<PairLabel>,
    pub canonical: Vec<CanonicalLabel>,
}

/// Read `queries.tsv` and `pairs.tsv` from a labeled-set directory.
///
/// Tab-separated rather than the testbed's `key=value`, because a query
/// contains spaces and quoting it would be one more thing to get wrong by
/// hand. Blank lines and `#` comments are skipped; an unparseable row fails
/// loudly, since a silently dropped label is an eval that overstates itself.
pub fn load(dir: &Path) -> Result<Labels> {
    let mut labels = Labels::default();

    for (line_no, line) in rows(&dir.join("queries.tsv"))? {
        let mut fields = line.split('\t').map(str::trim);
        let (Some(query), Some(expected)) = (fields.next(), fields.next()) else {
            bail!("queries.tsv:{line_no}: expected `query<TAB>Owner#method`");
        };
        if query.is_empty() || expected.is_empty() {
            bail!("queries.tsv:{line_no}: empty query or expectation");
        }
        labels.queries.push(QueryLabel {
            query: query.to_string(),
            expected: expected.to_string(),
            provisional: fields.next() == Some("provisional"),
        });
    }

    for (line_no, line) in rows(&dir.join("pairs.tsv"))? {
        let mut fields = line.split('\t').map(str::trim);
        let (Some(a), Some(b), Some(verdict)) = (fields.next(), fields.next(), fields.next())
        else {
            bail!("pairs.tsv:{line_no}: expected `a<TAB>b<TAB>duplicate|near|distinct`");
        };
        let verdict = match verdict {
            "duplicate" => Verdict::Duplicate,
            "near" => Verdict::Near,
            "distinct" => Verdict::Distinct,
            other => bail!("pairs.tsv:{line_no}: `{other}` is not duplicate, near, or distinct"),
        };
        labels.pairs.push(PairLabel {
            a: a.to_string(),
            b: b.to_string(),
            verdict,
            provisional: fields.next() == Some("provisional"),
        });
    }
    // Optional: a set may label duplicates without labelling canonicality, and
    // an absent file is a set that says nothing rather than a broken one.
    for (line_no, line) in optional_rows(&dir.join("canonical.tsv"))? {
        let mut fields = line.split('\t').map(str::trim);
        let (Some(canonical), Some(alternate), Some(evidence)) =
            (fields.next(), fields.next(), fields.next())
        else {
            bail!("canonical.tsv:{line_no}: expected `canonical<TAB>alternate<TAB>evidence`");
        };
        let evidence: Vec<String> = evidence.split(',').map(|e| e.trim().to_string()).collect();
        if let Some(unknown) = evidence.iter().find(|e| !EVIDENCE.contains(&e.as_str())) {
            bail!(
                "canonical.tsv:{line_no}: `{unknown}` is not one of {}",
                EVIDENCE.join(", ")
            );
        }
        labels.canonical.push(CanonicalLabel {
            canonical: canonical.to_string(),
            alternate: alternate.to_string(),
            evidence,
            provisional: fields.next() == Some("provisional"),
        });
    }
    Ok(labels)
}

fn optional_rows(path: &Path) -> Result<Vec<(usize, String)>> {
    match path.exists() {
        true => rows(path),
        false => Ok(Vec::new()),
    }
}

fn rows(path: &Path) -> Result<Vec<(usize, String)>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim().to_string()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .collect())
}

/// Hit-rate over a set of labeled queries.
#[derive(Debug, Default, serde::Serialize)]
pub struct Ranking {
    pub label: String,
    pub top1: usize,
    pub top5: usize,
    pub found: usize,
    pub total: usize,
    /// Labels whose expected unit is not in the index at all. Reported rather
    /// than counted as a miss: it is a broken label, not a bad answer.
    pub unknown: usize,
}

impl Ranking {
    fn rate(&self, n: usize) -> f64 {
        match self.total {
            0 => 0.0,
            total => (n as f64 / total as f64 * 100.0).round() / 100.0,
        }
    }
}

/// Duplicate detection against labeled pairs, at one `min_lines` setting.
#[derive(Debug, Default, serde::Serialize)]
pub struct Dupes {
    pub min_lines: u32,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub unknown: usize,
    /// The smallest labeled true duplicate, and the largest labeled distinct
    /// pair that collides anyway. Together these say where `--min-lines`
    /// belongs — the floor wants to sit above the second and below the first.
    pub smallest_true: Option<u32>,
    pub largest_false: Option<u32>,
}

/// What the cosines actually look like, and what a floor would cost.
#[derive(Debug, Default, serde::Serialize)]
pub struct Calibration {
    /// Cosine of the expected unit, per query that had one.
    pub answers: Distribution,
    /// Best-scoring *wrong* unit, per query. The population a floor has to
    /// separate the answers from.
    pub distractors: Distribution,
    pub sweep: Vec<FloorPoint>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Distribution {
    pub n: usize,
    pub min: f64,
    pub mean: f64,
    pub max: f64,
}

impl Distribution {
    fn of(values: &[f64]) -> Distribution {
        if values.is_empty() {
            return Distribution::default();
        }
        // Two decimals: a cosine off a 256-dim vector over a few dozen labels
        // has about that much meaning, and more digits invent precision.
        let round = |x: f64| (x * 100.0).round() / 100.0;
        Distribution {
            n: values.len(),
            min: round(values.iter().copied().fold(f64::INFINITY, f64::min)),
            mean: round(values.iter().sum::<f64>() / values.len() as f64),
            max: round(values.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct FloorPoint {
    pub floor: f64,
    /// Labeled answers still above the floor.
    pub answers_kept: usize,
    /// Distractors the floor removes.
    pub distractors_cut: usize,
}

/// How the canonicality signals did against labeled edges.
#[derive(Debug, Default, serde::Serialize)]
pub struct CanonicalScore {
    pub correct: usize,
    pub wrong: usize,
    /// The signals disagreed, or none could be measured. **Not** a wrong
    /// answer: abstaining is the designed behaviour (DEC-019), and scoring it
    /// as wrong would push the design toward guessing to raise a number.
    pub abstained: usize,
    /// A label naming a unit this index does not hold. A broken label, not a
    /// bad answer — the same convention `Ranking::unknown` uses.
    pub unknown: usize,
    /// Each signal on its own, which is the number that says whether a signal
    /// earns its cost. A signal that is right twice and silent thrice is a
    /// different proposition from one that is right twice and wrong thrice.
    pub signals: Vec<SignalScore>,
    pub cases: Vec<CanonicalCase>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SignalScore {
    pub signal: String,
    pub correct: usize,
    pub wrong: usize,
    /// Tied, or unavailable. Silence is honest; it is not a miss.
    pub silent: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct CanonicalCase {
    pub canonical: String,
    pub alternate: String,
    /// `correct`, `wrong`, `abstained`, or `unknown`.
    pub outcome: &'static str,
    pub basis: String,
}

#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub corpus: String,
    pub embedder: &'static str,
    pub coverage_state: &'static str,
    pub summarized: usize,
    pub summarizable: usize,
    pub rankings: Vec<Ranking>,
    pub dupes: Dupes,
    /// The near tier, scored on its own labels. Separate from `dupes` because
    /// the two answer different questions and share no threshold.
    pub near: Dupes,
    pub calibration: Calibration,
    pub canonical: CanonicalScore,
    pub provisional_queries: usize,
    pub provisional_pairs: usize,
    pub provisional_canonical: usize,
}

/// Run the whole eval against the current checkout.
pub fn run(
    store: &mut Store,
    root: &Path,
    labels: &Labels,
    embedder: &dyn Embedder,
    min_lines: u32,
) -> Result<Report> {
    let root_str = root.to_string_lossy().into_owned();
    // The corpus's own path policy, defaults included: the eval scores what a
    // user of this checkout would actually be shown (DEC-022).
    let classes = crate::paths::Classes::load(root)?;
    let units = store.units(&root_str)?;
    let by_id: HashMap<String, usize> = units
        .iter()
        .enumerate()
        // First occurrence wins: a unit at two paths is one answer.
        .fold(HashMap::new(), |mut acc, (i, u)| {
            acc.entry(u.unit.id()).or_insert(i);
            acc
        });

    // Deep enough that a label's rank is knowable rather than "somewhere past
    // the cutoff" — a miss and a rank-200 hit are different facts.
    let depth = units.len().max(1);
    let mut coverage_state = "none";
    let (mut summarized, mut summarizable) = (0, 0);
    let mut calibration_answers: Vec<f64> = Vec::new();
    let mut calibration_distractors: Vec<f64> = Vec::new();

    // Both tiers, on one corpus: the embed-the-code against embed-the-summary
    // comparison DEC-004 promised. `Best` uses a summary wherever one exists
    // and falls back to identifiers; `IdentifierOnly` forces the free tier, so
    // the difference between the two rows is exactly what summaries bought.
    let mut ranked: Vec<Ranking> = Vec::new();
    for (label, prefer) in [
        ("contour", crate::search::Prefer::Best),
        ("contour:identifier", crate::search::Prefer::IdentifierOnly),
    ] {
        let mut ranking = Ranking {
            label: label.to_string(),
            ..Ranking::default()
        };
        let mut answers: Vec<f64> = Vec::new();
        let mut distractors: Vec<f64> = Vec::new();

        for query in &labels.queries {
            if !by_id.contains_key(&query.expected) {
                ranking.unknown += 1;
                continue;
            }
            ranking.total += 1;
            // No floor: this run exists to decide where the floor belongs, and
            // measuring above an existing one can only confirm it.
            let answer = crate::search::search(
                store,
                &root_str,
                &query.query,
                embedder,
                crate::search::Options {
                    limit: depth,
                    prefer,
                    ..crate::search::Options::new(&classes)
                },
            )?;
            debug_assert_eq!(answer.withheld, 0, "the eval must run unfloored");
            coverage_state = answer.coverage_state;
            summarized = answer.coverage.summarized;
            summarizable = answer.coverage.summarizable;

            match answer.hits.iter().position(|h| h.id == query.expected) {
                Some(0) => {
                    ranking.top1 += 1;
                    ranking.top5 += 1;
                    ranking.found += 1;
                }
                Some(r) if r < 5 => {
                    ranking.top5 += 1;
                    ranking.found += 1;
                }
                Some(_) => ranking.found += 1,
                None => {}
            }

            // The two populations a floor has to separate.
            if let Some(cosine) = answer
                .hits
                .iter()
                .find(|h| h.id == query.expected)
                .and_then(|h| h.cosine)
            {
                answers.push(cosine);
            }
            if let Some(best) = answer
                .hits
                .iter()
                .filter(|h| h.id != query.expected)
                .filter_map(|h| h.cosine)
                .fold(None, |acc: Option<f64>, c| {
                    Some(acc.map_or(c, |a| a.max(c)))
                })
            {
                distractors.push(best);
            }
        }
        // The floor is calibrated for the tier a user actually gets.
        if prefer == crate::search::Prefer::Best {
            calibration_answers = answers;
            calibration_distractors = distractors;
        }
        ranked.push(ranking);
    }

    let mut rankings = ranked;
    rankings.push(baseline(
        "baseline:name",
        labels,
        &units,
        &by_id,
        |u, _| crate::embed::humanize(&u.unit.id()),
    )?);
    let sources = read_sources(root, &units);
    rankings.push(baseline(
        "baseline:source",
        labels,
        &units,
        &by_id,
        |_, i| sources.get(&i).cloned().unwrap_or_default(),
    )?);

    let dupes = score_dupes(store, &root_str, labels, min_lines, &classes)?;
    let near = score_near(store, &root_str, labels, min_lines, &classes)?;
    Ok(Report {
        corpus: root_str,
        embedder: embedder.kind(),
        coverage_state,
        summarized,
        summarizable,
        rankings,
        dupes,
        near,
        calibration: Calibration {
            sweep: sweep(&calibration_answers, &calibration_distractors),
            answers: Distribution::of(&calibration_answers),
            distractors: Distribution::of(&calibration_distractors),
        },
        canonical: score_canonical(root, labels, &units, &by_id),
        provisional_queries: labels.queries.iter().filter(|q| q.provisional).count(),
        provisional_pairs: labels.pairs.iter().filter(|p| p.provisional).count(),
        provisional_canonical: labels.canonical.iter().filter(|c| c.provisional).count(),
    })
}

/// A token-overlap ranker over whatever text `text_of` yields.
///
/// This is the honest version of "what would a developer get with `rg`": it
/// sees the raw text, scores by how many query tokens appear in it, and breaks
/// ties toward smaller units. The source variant is deliberately *generous* —
/// it reads the method body, which contour's semantic half never does.
fn baseline(
    label: &str,
    labels: &Labels,
    units: &[Located],
    by_id: &HashMap<String, usize>,
    text_of: impl Fn(&Located, usize) -> String,
) -> Result<Ranking> {
    let texts: Vec<Vec<String>> = units
        .iter()
        .enumerate()
        .map(|(i, u)| tokens(&text_of(u, i)))
        .collect();

    let mut ranking = Ranking {
        label: label.to_string(),
        ..Ranking::default()
    };
    for query_label in &labels.queries {
        let Some(&expected) = by_id.get(&query_label.expected) else {
            ranking.unknown += 1;
            continue;
        };
        ranking.total += 1;
        let query = tokens(&query_label.query);
        let mut scored: Vec<(usize, usize, u32)> = texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let hits = query.iter().filter(|t| text.contains(t)).count();
                (i, hits, units[i].unit.end_line + 1 - units[i].unit.line)
            })
            .filter(|(_, hits, _)| *hits > 0)
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));

        match scored.iter().position(|(i, _, _)| *i == expected) {
            Some(0) => {
                ranking.top1 += 1;
                ranking.top5 += 1;
                ranking.found += 1;
            }
            Some(r) if r < 5 => {
                ranking.top5 += 1;
                ranking.found += 1;
            }
            Some(_) => ranking.found += 1,
            None => {}
        }
    }
    Ok(ranking)
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn read_sources(root: &Path, units: &[Located]) -> HashMap<usize, String> {
    let mut files: HashMap<&str, Vec<String>> = HashMap::new();
    let mut out = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        let lines = files.entry(&unit.path).or_insert_with(|| {
            std::fs::read_to_string(root.join(&unit.path))
                .map(|t| t.lines().map(str::to_string).collect())
                .unwrap_or_default()
        });
        let (from, to) = (unit.unit.line as usize - 1, unit.unit.end_line as usize);
        if from < lines.len() && to <= lines.len() {
            out.insert(i, lines[from..to].join("\n"));
        }
    }
    out
}

fn score_dupes(
    store: &Store,
    root: &str,
    labels: &Labels,
    min_lines: u32,
    classes: &crate::paths::Classes,
) -> Result<Dupes> {
    // Every pair the tool currently reports, at no floor — so the labeled
    // pairs can be scored against the raw signal and the floor swept
    // separately.
    let found = crate::dupes::find(store, root, None, 1, classes)?;
    let mut reported: HashMap<(&str, &str), u32> = HashMap::new();
    for group in &found.groups {
        for (i, a) in group.members.iter().enumerate() {
            for b in &group.members[i + 1..] {
                reported.insert((a.id.as_str(), b.id.as_str()), group.lines);
                reported.insert((b.id.as_str(), a.id.as_str()), group.lines);
            }
        }
    }

    let mut out = Dupes {
        min_lines,
        ..Dupes::default()
    };
    let known: std::collections::HashSet<String> =
        store.units(root)?.iter().map(|u| u.unit.id()).collect();

    for pair in &labels.pairs {
        if !known.contains(&pair.a) || !known.contains(&pair.b) {
            out.unknown += 1;
            continue;
        }
        let collides = reported.get(&(pair.a.as_str(), pair.b.as_str())).copied();
        // A `near` label must NOT be reported by the exact tier, so it scores
        // here as a distinct pair does.
        match (pair.verdict == Verdict::Duplicate, collides) {
            (true, Some(lines)) if lines >= min_lines => {
                out.true_positives += 1;
                out.smallest_true = Some(out.smallest_true.map_or(lines, |m: u32| m.min(lines)));
            }
            // Collides, but the floor hides it: still a miss for the user.
            (true, _) => out.false_negatives += 1,
            (false, Some(lines)) if lines >= min_lines => {
                out.false_positives += 1;
                out.largest_false = Some(out.largest_false.map_or(lines, |m: u32| m.max(lines)));
            }
            (false, _) => {}
        }
    }
    Ok(out)
}

/// The near tier against its own labels.
///
/// Both edges are asserted, which is what makes the threshold calibrated
/// rather than chosen: a `near` label below it is a false negative, and a
/// `distinct` label above it is a false positive. The `super` pairs
/// (DEC-017) are the negatives that matter — they score 0.63 and 0.67, and
/// the threshold has to stay clear of them.
fn score_near(
    store: &Store,
    root: &str,
    labels: &Labels,
    min_lines: u32,
    classes: &crate::paths::Classes,
) -> Result<Dupes> {
    let (found, _) = crate::dupes::find_near(
        store,
        root,
        None,
        min_lines,
        crate::near::NEAR_THRESHOLD,
        classes,
    )?;
    let mut reported: HashMap<(&str, &str), u32> = HashMap::new();
    for group in &found.groups {
        let (a, b) = (&group.members[0], &group.members[1]);
        let lines = group.lines;
        reported.insert((a.id.as_str(), b.id.as_str()), lines);
        reported.insert((b.id.as_str(), a.id.as_str()), lines);
    }

    let known: std::collections::HashSet<String> =
        store.units(root)?.iter().map(|u| u.unit.id()).collect();
    let mut out = Dupes {
        min_lines,
        ..Dupes::default()
    };
    for pair in &labels.pairs {
        // A `duplicate` label is the exact tier's business; the near tier is
        // scored only on what it is responsible for.
        if pair.verdict == Verdict::Duplicate {
            continue;
        }
        if !known.contains(&pair.a) || !known.contains(&pair.b) {
            out.unknown += 1;
            continue;
        }
        let found = reported.get(&(pair.a.as_str(), pair.b.as_str())).copied();
        match (pair.verdict, found) {
            (Verdict::Near, Some(lines)) => {
                out.true_positives += 1;
                out.smallest_true = Some(out.smallest_true.map_or(lines, |m: u32| m.min(lines)));
            }
            (Verdict::Near, None) => out.false_negatives += 1,
            (Verdict::Distinct, Some(lines)) => {
                out.false_positives += 1;
                out.largest_false = Some(out.largest_false.map_or(lines, |m: u32| m.max(lines)));
            }
            (Verdict::Distinct, None) => {}
            (Verdict::Duplicate, _) => unreachable!("skipped above"),
        }
    }
    Ok(out)
}

/// The canonicality signals against labeled edges.
///
/// Ranks each labeled pair directly rather than looking for a duplicate group
/// that holds it, because a `delegates` edge has two different bodies and no
/// group will ever hold it — and the harness must be able to say whether the
/// signals get that one right too.
fn score_canonical(
    root: &Path,
    labels: &Labels,
    units: &[Located],
    by_id: &HashMap<String, usize>,
) -> CanonicalScore {
    let mut out = CanonicalScore::default();
    if labels.canonical.is_empty() {
        return out;
    }
    let candidate = |id: &str| -> Option<crate::canonical::Candidate> {
        let unit = &units[*by_id.get(id)?];
        Some(crate::canonical::Candidate {
            id: id.to_string(),
            path: unit.path.clone(),
            line: unit.unit.line,
            end_line: unit.unit.end_line,
            lang: unit.unit.lang,
        })
    };

    // One batch of probes for every labeled unit, so a name in two edges is
    // blamed once.
    let pairs: Vec<(&CanonicalLabel, Vec<crate::canonical::Candidate>)> = labels
        .canonical
        .iter()
        .map(|label| {
            let both = [candidate(&label.canonical), candidate(&label.alternate)];
            (label, both.into_iter().flatten().collect())
        })
        .collect();
    let all: Vec<crate::canonical::Candidate> =
        pairs.iter().flat_map(|(_, c)| c.iter().cloned()).collect();
    let probes = crate::canonical::Probes::gather(root, &all);

    let mut signals: HashMap<String, SignalScore> = HashMap::new();
    for (label, candidates) in &pairs {
        if candidates.len() != 2 {
            out.unknown += 1;
            out.cases.push(CanonicalCase {
                canonical: label.canonical.clone(),
                alternate: label.alternate.clone(),
                outcome: "unknown",
                basis: "one side is not in this index".into(),
            });
            continue;
        }
        let ranked = probes.rank(candidates);
        for signal in &ranked.signals {
            let score = signals
                .entry(signal.signal.to_string())
                .or_insert(SignalScore {
                    signal: signal.signal.to_string(),
                    ..SignalScore::default()
                });
            match signal.picks.as_ref().map(|p| p.id.as_str()) {
                Some(pick) if pick == label.canonical => score.correct += 1,
                Some(_) => score.wrong += 1,
                None => score.silent += 1,
            }
        }
        let outcome = match ranked.pick.as_ref().map(|p| p.id.as_str()) {
            Some(pick) if pick == label.canonical => {
                out.correct += 1;
                "correct"
            }
            Some(_) => {
                out.wrong += 1;
                "wrong"
            }
            None => {
                out.abstained += 1;
                "abstained"
            }
        };
        out.cases.push(CanonicalCase {
            canonical: label.canonical.clone(),
            alternate: label.alternate.clone(),
            outcome,
            basis: ranked.basis,
        });
    }
    // Signal order follows the ranker's, so the report reads the same way the
    // reasoning does.
    for name in ["git_age", "references", "namespace_depth"] {
        if let Some(score) = signals.remove(name) {
            out.signals.push(score);
        }
    }
    out
}

/// What each candidate floor would keep and cut. The point of the eval.
fn sweep(answers: &[f64], distractors: &[f64]) -> Vec<FloorPoint> {
    (0..=10)
        .map(|step| {
            let floor = step as f64 / 20.0; // 0.00 … 0.50
            FloorPoint {
                floor,
                answers_kept: answers.iter().filter(|c| **c >= floor).count(),
                distractors_cut: distractors.iter().filter(|c| **c < floor).count(),
            }
        })
        .collect()
}

/// Human rendering. Every rate carries its fraction, so a reader can see the
/// denominator that a percentage hides.
pub fn render(report: &Report) {
    println!(
        "corpus     {}\nembedder   {}   coverage {} ({}/{} summarized)",
        crate::paths::pretty(&report.corpus),
        report.embedder,
        report.coverage_state,
        report.summarized,
        report.summarizable
    );
    println!("\nsearch");
    for r in &report.rankings {
        println!(
            "  {:<18} top1 {:.2} ({}/{})   top5 {:.2} ({}/{})   found {}/{}",
            r.label,
            r.rate(r.top1),
            r.top1,
            r.total,
            r.rate(r.top5),
            r.top5,
            r.total,
            r.found,
            r.total
        );
    }
    if let Some(unknown) = report.rankings.first().map(|r| r.unknown)
        && unknown > 0
    {
        println!("  {unknown} label(s) name a unit not in this index");
    }

    let d = &report.dupes;
    // `n/a` rather than `NaN`: a tier with no labels has no precision, and
    // printing a float's error value as though it were a measurement is the
    // thing DEC-010 exists to stop.
    let rate = |n: usize, total: usize| match total {
        0 => "n/a".to_string(),
        t => format!("{:.2}", (n as f64 / t as f64 * 100.0).round() / 100.0),
    };
    println!("\nduplicates (min_lines {})", d.min_lines);
    println!(
        "  precision {} ({}/{})   recall {} ({}/{})",
        rate(d.true_positives, d.true_positives + d.false_positives),
        d.true_positives,
        d.true_positives + d.false_positives,
        rate(d.true_positives, d.true_positives + d.false_negatives),
        d.true_positives,
        d.true_positives + d.false_negatives
    );
    // Where `--min-lines` belongs: above the largest false collision, below
    // the smallest true duplicate. Printed even when only one side exists,
    // because "no false collisions at any size" is itself the finding.
    match (d.smallest_true, d.largest_false) {
        (Some(t), Some(f)) => {
            println!("  smallest true duplicate {t} lines, largest false collision {f} lines")
        }
        (Some(t), None) => {
            println!("  smallest true duplicate {t} lines, no false collisions at any size")
        }
        (None, Some(f)) => println!("  largest false collision {f} lines, no true ones found"),
        (None, None) => {}
    }

    let c = &report.calibration;
    let n = &report.near;
    println!(
        "\nnear-structural (jaccard >= {:.2})",
        crate::near::NEAR_THRESHOLD
    );
    println!(
        "  precision {} ({}/{})   recall {} ({}/{})",
        rate(n.true_positives, n.true_positives + n.false_positives),
        n.true_positives,
        n.true_positives + n.false_positives,
        rate(n.true_positives, n.true_positives + n.false_negatives),
        n.true_positives,
        n.true_positives + n.false_negatives
    );

    println!("\ncalibration");
    println!(
        "  answers      n {:<4} min {:.2}  mean {:.2}  max {:.2}",
        c.answers.n, c.answers.min, c.answers.mean, c.answers.max
    );
    println!(
        "  distractors  n {:<4} min {:.2}  mean {:.2}  max {:.2}",
        c.distractors.n, c.distractors.min, c.distractors.mean, c.distractors.max
    );
    println!("  floor   answers kept   distractors cut");
    for point in &c.sweep {
        println!(
            "  {:.2}    {:>3}/{:<9} {:>3}/{}",
            point.floor, point.answers_kept, c.answers.n, point.distractors_cut, c.distractors.n
        );
    }
    let canon = &report.canonical;
    if !canon.cases.is_empty() {
        println!("\ncanonicality ({} labeled edge(s))", canon.cases.len());
        // Abstentions stand apart from wrong answers on purpose: refusing to
        // pick when the signals disagree is the design (DEC-019), and a report
        // that folded the two would make guessing look like an improvement.
        println!(
            "  correct {}   wrong {}   abstained {}   unknown {}",
            canon.correct, canon.wrong, canon.abstained, canon.unknown
        );
        println!("  signal            correct  wrong  silent");
        for signal in &canon.signals {
            println!(
                "  {:<16} {:>7} {:>6} {:>7}",
                signal.signal, signal.correct, signal.wrong, signal.silent
            );
        }
        // Only what did not land: the correct rows say nothing a reader acts
        // on, and the list stays short exactly when the signals are working.
        for case in canon.cases.iter().filter(|case| case.outcome != "correct") {
            println!(
                "  [{}] {} over {} — {}",
                case.outcome, case.canonical, case.alternate, case.basis
            );
        }
    }

    let provisional =
        report.provisional_queries + report.provisional_pairs + report.provisional_canonical;
    if provisional > 0 {
        println!(
            "\n{} query, {} pair and {} canonical label(s) are marked provisional \
             — the labeller was unsure, and they are included above.",
            report.provisional_queries, report.provisional_pairs, report.provisional_canonical
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sweep_shows_what_a_floor_would_cost() {
        let points = sweep(&[0.30, 0.45, 0.60], &[0.10, 0.35]);
        let at = |f: f64| points.iter().find(|p| (p.floor - f).abs() < 1e-6).unwrap();
        assert_eq!((at(0.0).answers_kept, at(0.0).distractors_cut), (3, 0));
        // 0.40 loses one real answer and removes both distractors.
        assert_eq!((at(0.40).answers_kept, at(0.40).distractors_cut), (2, 2));
        assert_eq!((at(0.50).answers_kept, at(0.50).distractors_cut), (1, 2));
    }

    /// A typo in the evidence column must fail the run, not become a label
    /// that quietly means something else. The same rule the verdict column has.
    #[test]
    fn a_canonical_label_with_unknown_evidence_fails_loudly() {
        let dir = std::env::temp_dir().join(format!("contour-labels-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("queries.tsv"), "").unwrap();
        std::fs::write(dir.join("pairs.tsv"), "").unwrap();

        std::fs::write(dir.join("canonical.tsv"), "A#x\tB#x\tolder,delegates\n").unwrap();
        let labels = load(&dir).unwrap();
        assert_eq!(labels.canonical.len(), 1);
        assert_eq!(labels.canonical[0].evidence, ["older", "delegates"]);

        std::fs::write(dir.join("canonical.tsv"), "A#x\tB#x\tolder,oldre\n").unwrap();
        assert!(load(&dir).is_err(), "`oldre` is not evidence");

        // An absent file is a set that says nothing about canonicality, not a
        // broken one — the fixture set has no such labels and still runs.
        std::fs::remove_file(dir.join("canonical.tsv")).unwrap();
        assert!(load(&dir).unwrap().canonical.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_distribution_rounds_to_the_precision_it_has() {
        let d = Distribution::of(&[0.1234, 0.5678]);
        assert_eq!((d.n, d.min, d.max), (2, 0.12, 0.57));
        assert_eq!(d.mean, 0.35);
        assert_eq!(Distribution::of(&[]).n, 0);
    }
}
