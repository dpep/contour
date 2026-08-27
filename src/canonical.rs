//! Which member of a duplicate group is likely the original.
//!
//! A `dupes` group says these bodies are the same. The question a reader
//! actually has next is *which one do I keep*, and the answer is not in the
//! bodies — they are identical. It is outside them: which appeared first, which
//! the rest of the codebase calls, where each sits in the namespace.
//!
//! ## No composite score
//!
//! Each signal is measured independently and **reports its own pick**. Nothing
//! here adds, weights, or normalizes them into one number, because a number
//! built from a date and a call count is a number that means nothing and looks
//! like it means something (DEC-010). When the signals agree, that agreement is
//! the answer. When they disagree, saying so *is* the finding: it usually means
//! the old one was superseded and nobody deleted it, which is exactly what a
//! reader needs to know before consolidating.
//!
//! One deliberate asymmetry: `git_age` and `references` are **deciding**
//! signals, and `namespace_depth` is a **tiebreak** consulted only when both
//! are silent. Depth is a weak signal, and letting it disagree its way into an
//! abstention would make abstention the usual answer. The distinction is a
//! field in the output rather than a hidden weight, so a reader can see the
//! rule instead of inferring it.
//!
//! ## What it ranks
//!
//! A set of [`Candidate`]s, not a duplicate group. A group is the usual source
//! of one — but a shim that *delegates* to the implementation it shadows has a
//! different body, so the dupes tier never sees the pair, and it is exactly the
//! pair a reader most wants ranked. The labeled set proved that: four of its
//! five canonical rows are structural clones and the fifth is a delegation.
//!
//! ## Why this shells out
//!
//! Both deciding signals are questions somebody else's tool already answers
//! well: git owns history, and trekr owns "who really calls this method" for
//! Ruby, receiver-tiered in a way no grep can be. Reimplementing either inside
//! contour would be a second, worse copy. The cost is that both can be absent —
//! so both degrade to `unavailable` with the reason attached, and never to a
//! guess.

use crate::core::Lang;
use crate::dupes::Group;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// A signal that settles the pick when it has an opinion.
const DECIDING: &str = "deciding";
/// A signal consulted only when every deciding signal is silent.
const TIEBREAK: &str = "tiebreak";

/// One implementation under consideration.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub path: String,
    pub line: u32,
    pub end_line: u32,
    pub lang: Lang,
}

impl Candidate {
    /// What identifies one body to `git blame`.
    fn span(&self) -> (String, u32, u32) {
        (self.path.clone(), self.line, self.end_line)
    }
}

/// The likely-canonical member of one candidate set, and the basis for saying
/// so.
#[derive(Debug, serde::Serialize)]
pub struct Canonical {
    /// The member every deciding signal that spoke agrees on. `None` when they
    /// disagree, and when nothing could be measured at all — both of which are
    /// answers rather than failures, spelled out in [`Canonical::basis`].
    pub pick: Option<String>,
    /// What each signal measured and what it said, in one sentence. Never a
    /// score: a reader has to be able to disagree with the reasoning, which
    /// requires seeing it.
    pub basis: String,
    pub signals: Vec<Signal>,
}

/// One measurement across a group's members, and the member it favours.
#[derive(Debug, serde::Serialize)]
pub struct Signal {
    pub signal: &'static str,
    /// `deciding` or `tiebreak`. In the output rather than hidden in the code,
    /// so a reader can see why the shallowest namespace did not outvote the
    /// oldest commit.
    pub weight: &'static str,
    /// `measured` — it has a pick; `tied` — it measured every member and they
    /// came out equal; `unavailable` — it could not measure at all.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picks: Option<String>,
    /// What this signal found, or why it found nothing. The clause the group's
    /// `basis` is built from, kept per signal so a consumer reading one signal
    /// does not have to re-derive it.
    pub note: String,
    /// Per member, in the group's own order. Empty when unavailable.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measured: Vec<Measured>,
}

/// One member's value for one signal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Measured {
    pub id: String,
    /// The number the pick is made from. Comparable within one signal and
    /// meaningless across them, which is the whole reason nothing sums them.
    pub value: i64,
    /// The same number as a person wants to read it.
    pub display: String,
}

/// What the probes cost, so the price of `--canonical` is auditable rather
/// than asserted.
#[derive(Debug, Default, serde::Serialize)]
pub struct Stats {
    /// Distinct `git blame` invocations. One per body span, however many
    /// candidate sets mention it.
    pub git_probes: usize,
    /// Distinct `trekr --refs` invocations, one per Ruby unit name.
    pub trekr_probes: usize,
    pub millis: u128,
}

/// Every measurement, taken once and shared across every ranking that needs it.
///
/// Gathered up front rather than lazily per set, because these are process
/// calls: a body appearing in an exact group and in a near pair is blamed once,
/// and the whole batch runs in parallel instead of a serial walk with
/// duplicates in it.
pub struct Probes {
    ages: HashMap<(String, u32, u32), Result<i64, String>>,
    refs: HashMap<String, Refs>,
    pub stats: Stats,
}

impl Probes {
    /// Measure everything the given candidates could need.
    pub fn gather(root: &Path, candidates: &[Candidate]) -> Probes {
        let started = std::time::Instant::now();
        let mut spans: Vec<(String, u32, u32)> = candidates.iter().map(Candidate::span).collect();
        let mut names: Vec<String> = candidates
            .iter()
            .filter(|c| c.lang == Lang::Ruby)
            .map(|c| c.id.clone())
            .collect();
        spans.sort();
        spans.dedup();
        names.sort();
        names.dedup();

        let ages: HashMap<(String, u32, u32), Result<i64, String>> = spans
            .par_iter()
            .map(|key| (key.clone(), oldest_line(root, &key.0, key.1, key.2)))
            .collect();
        let refs: HashMap<String, Refs> = names
            .par_iter()
            .map(|id| (id.clone(), references(root, id)))
            .collect();

        Probes {
            stats: Stats {
                git_probes: ages.len(),
                trekr_probes: refs.len(),
                millis: started.elapsed().as_millis(),
            },
            ages,
            refs,
        }
    }

    /// Which of these is likely the canonical one.
    pub fn rank(&self, candidates: &[Candidate]) -> Canonical {
        let signals = vec![
            git_age(candidates, &self.ages),
            reference_count(candidates, &self.refs),
            namespace_depth(candidates),
        ];
        let (pick, basis) = resolve(&signals);
        Canonical {
            pick,
            basis,
            signals,
        }
    }
}

/// Annotate every group with its likely-canonical member.
pub fn annotate(root: &Path, groups: &mut [Group]) -> Result<Stats> {
    let candidates: Vec<Candidate> = groups
        .iter()
        .flat_map(|g| {
            g.members.iter().map(|m| Candidate {
                id: m.id.clone(),
                path: m.path.clone(),
                line: m.line,
                end_line: m.end_line,
                lang: g.lang,
            })
        })
        .collect();
    let probes = Probes::gather(root, &candidates);

    let mut at = 0;
    for group in groups.iter_mut() {
        let members = &candidates[at..at + group.members.len()];
        at += group.members.len();
        group.canonical = Some(probes.rank(members));
    }
    Ok(probes.stats)
}

/// Agreement, disagreement, or silence — and the sentence for each.
fn resolve(signals: &[Signal]) -> (Option<String>, String) {
    let spoke: Vec<&Signal> = signals
        .iter()
        .filter(|s| s.weight == DECIDING && s.picks.is_some())
        .collect();
    let clauses = |group: &[&Signal]| -> String {
        group
            .iter()
            .map(|s| s.note.clone())
            .collect::<Vec<_>>()
            .join("; ")
    };

    match spoke.first() {
        // Every deciding signal that spoke named the same member.
        Some(first) if spoke.iter().all(|s| s.picks == first.picks) => {
            (first.picks.clone(), clauses(&spoke))
        }
        // They named different members. That is a finding — usually a
        // superseded implementation nobody deleted — so it is reported as one
        // rather than resolved by a rule that would be inventing an answer.
        Some(_) => (None, format!("signals disagree — {}", clauses(&spoke))),
        None => {
            let silent = clauses(
                &signals
                    .iter()
                    .filter(|s| s.weight == DECIDING)
                    .collect::<Vec<_>>(),
            );
            match signals
                .iter()
                .find(|s| s.weight == TIEBREAK && s.picks.is_some())
            {
                Some(weak) => (
                    weak.picks.clone(),
                    format!(
                        "no deciding signal ({silent}); {} — a weak basis",
                        weak.note
                    ),
                ),
                None => (None, format!("nothing to go on — {silent}")),
            }
        }
    }
}

/// When each member's oldest surviving line was written.
///
/// *Oldest surviving*, not *introduced*: blame reports the last commit to
/// touch each line, so a body nobody has edited dates to when it was written,
/// and one reformatted last week dates to last week. Stated plainly because it
/// is the signal's real limitation — a wholesale reformat makes an old
/// implementation look new, and there is no cheap measurement that does not.
fn git_age(
    candidates: &[Candidate],
    ages: &HashMap<(String, u32, u32), Result<i64, String>>,
) -> Signal {
    let mut measured = Vec::new();
    for member in candidates {
        let key = member.span();
        match ages.get(&key) {
            Some(Ok(at)) => measured.push(Measured {
                id: member.id.clone(),
                value: *at,
                display: date(*at),
            }),
            // One unreadable member makes the whole comparison unsound: a
            // group ranked on the members git could see would silently be
            // answering a different question.
            Some(Err(why)) => {
                return unavailable(
                    "git_age",
                    DECIDING,
                    format!("git history unavailable ({why})"),
                );
            }
            None => {
                return unavailable("git_age", DECIDING, "git history unavailable".into());
            }
        }
    }
    match winner(&measured, true) {
        Some(best) => {
            let runner_up = measured
                .iter()
                .filter(|m| m.id != best.id)
                .map(|m| m.value)
                .min()
                .unwrap_or(best.value);
            // "oldest surviving line", not "oldest": rails' two XmlMini
            // backends are a year apart in history and five days apart by this
            // measurement, because both were reformatted since. The phrasing is
            // the disclosure — a reader who knows what was measured can discount
            // it, and one told "oldest by 5 days" cannot.
            let note = format!("oldest surviving line by {}", span(runner_up - best.value));
            Signal {
                signal: "git_age",
                weight: DECIDING,
                status: "measured",
                picks: Some(best.id.clone()),
                note,
                measured,
            }
        }
        None => Signal {
            signal: "git_age",
            weight: DECIDING,
            status: "tied",
            picks: None,
            note: "same oldest line".into(),
            measured,
        },
    }
}

/// How much of the codebase calls each member, via trekr.
///
/// trekr tiers a call site as `confirmed` (the receiver is known to be this
/// owner) or `possible` (an untyped receiver nothing rules out). Confirmed
/// decides when anything has one; otherwise the comparison falls back to
/// possible and **says which tier it used**, because in idiomatic Ruby every
/// count is often zero-confirmed and collapsing the two tiers would report a
/// weaker measurement under a stronger name.
fn reference_count(candidates: &[Candidate], refs: &HashMap<String, Refs>) -> Signal {
    if let Some(other) = candidates.iter().find(|c| c.lang != Lang::Ruby) {
        return unavailable(
            "references",
            DECIDING,
            format!("trekr reads Ruby, and this is {}", other.lang.as_str()),
        );
    }
    let mut counts = Vec::new();
    for member in candidates {
        match refs.get(&member.id) {
            Some(Refs::Counts {
                confirmed,
                possible,
            }) => counts.push((member.id.clone(), *confirmed, *possible)),
            Some(Refs::Unavailable(why)) => {
                return unavailable("references", DECIDING, why.clone());
            }
            None => return unavailable("references", DECIDING, "not measured".into()),
        }
    }

    let tier_is_confirmed = counts.iter().any(|(_, confirmed, _)| *confirmed > 0);
    let measured: Vec<Measured> = counts
        .iter()
        .map(|(id, confirmed, possible)| {
            let (value, label) = match tier_is_confirmed {
                true => (*confirmed, "confirmed"),
                false => (*possible, "possible"),
            };
            Measured {
                id: id.clone(),
                value,
                display: format!("{value} {label}"),
            }
        })
        .collect();

    match winner(&measured, false) {
        Some(best) => {
            let rest: Vec<String> = measured
                .iter()
                .filter(|m| m.id != best.id)
                .map(|m| m.value.to_string())
                .collect();
            let note = format!(
                "{} reference{} against {}",
                best.display,
                if best.value == 1 { "" } else { "s" },
                rest.join(", ")
            );
            Signal {
                signal: "references",
                weight: DECIDING,
                status: "measured",
                picks: Some(best.id.clone()),
                note,
                measured,
            }
        }
        None => Signal {
            signal: "references",
            weight: DECIDING,
            status: "tied",
            picks: None,
            note: format!("{} each", measured[0].display),
            measured,
        },
    }
}

/// How deep in the namespace each member sits.
///
/// The weak one, and a tiebreak only. A shallower owner is more often the
/// general implementation and a deeper one the specialisation — `Payroll::Run`
/// against `Payroll::Import::Legacy::Run` — but the correlation is loose
/// enough that it must never overrule a date or a call count.
fn namespace_depth(candidates: &[Candidate]) -> Signal {
    let measured: Vec<Measured> = candidates
        .iter()
        .map(|m| {
            // Everything before the last separator is the owner; a top-level
            // callable has none and sits at depth 0.
            let owner =
                m.id.rsplit_once(['#', '.'])
                    .map(|(owner, _)| owner)
                    .unwrap_or("");
            let depth = match owner.is_empty() {
                true => 0,
                false => owner.split("::").count() as i64,
            };
            Measured {
                id: m.id.clone(),
                value: depth,
                display: format!("depth {depth}"),
            }
        })
        .collect();

    match winner(&measured, true) {
        Some(best) => Signal {
            signal: "namespace_depth",
            weight: TIEBREAK,
            status: "measured",
            picks: Some(best.id.clone()),
            note: format!("shallowest namespace at {}", best.display),
            measured,
        },
        None => Signal {
            signal: "namespace_depth",
            weight: TIEBREAK,
            status: "tied",
            picks: None,
            note: "same namespace depth".into(),
            measured,
        },
    }
}

fn unavailable(signal: &'static str, weight: &'static str, why: String) -> Signal {
    Signal {
        signal,
        weight,
        status: "unavailable",
        picks: None,
        note: why,
        measured: Vec::new(),
    }
}

/// The one member holding the extreme value, or `None` when two share it.
///
/// A tie is silence rather than a coin flip: picking the first of two equal
/// members would be an arbitrary choice reported in the same field as a
/// measured one.
fn winner(measured: &[Measured], lower_wins: bool) -> Option<&Measured> {
    let best = measured
        .iter()
        .map(|m| m.value)
        .reduce(|a, b| if (b < a) == lower_wins { b } else { a })?;
    let mut holders = measured.iter().filter(|m| m.value == best);
    let first = holders.next()?;
    holders.next().is_none().then_some(first)
}

/// When the oldest line of one body was last written, as unix seconds.
fn oldest_line(root: &Path, path: &str, line: u32, end_line: u32) -> Result<i64, String> {
    // `--porcelain` (not `--line-porcelain`) prints each commit's header once
    // and then just its sha, which is all this needs: the minimum over the
    // commits seen is the minimum over the lines.
    let out = Command::new("git")
        .args([
            "blame",
            "-L",
            &format!("{line},{end_line}"),
            "--porcelain",
            "--",
            path,
        ])
        .current_dir(root)
        .output()
        .map_err(|err| format!("running git blame: {err}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("git blame failed")
            .trim()
            .to_string());
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("author-time "))
        .filter_map(|t| t.trim().parse::<i64>().ok())
        .min()
        .ok_or_else(|| "git blame reported no commit".to_string())
}

/// What trekr said about one unit, or why it said nothing.
#[derive(Debug, Clone)]
enum Refs {
    Counts { confirmed: i64, possible: i64 },
    Unavailable(String),
}

/// Ask trekr who calls one Ruby unit.
///
/// `$CONTOUR_TREKR` names the binary, so a checkout can be annotated without
/// it — set it to something unrunnable and the signal reports itself absent,
/// which is also how the tests stay hermetic.
fn references(root: &Path, id: &str) -> Refs {
    let binary = std::env::var("CONTOUR_TREKR").unwrap_or_else(|_| "trekr".to_string());
    let out = match Command::new(&binary)
        .args(["--refs", id, "-j"])
        .current_dir(root)
        .output()
    {
        Ok(out) => out,
        Err(_) => return Refs::Unavailable(format!("`{binary}` is not installed")),
    };
    // trekr exits 1 for "nothing matched", which is a real count of zero, and
    // 2 for "cannot answer" — both carrying JSON that says which. So the exit
    // code is read from the payload rather than from the status.
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return Refs::Unavailable("trekr returned no answer".into());
    };
    if let Some(reason) = value["reason"].as_str() {
        // trekr says how to fix what it could not answer. Passing that through
        // turns "signal unavailable" into something a reader can act on, which
        // is the difference between a disclosure and an excuse.
        return Refs::Unavailable(match value["hint"].as_str() {
            Some(hint) => format!("trekr: {reason} — `{hint}`"),
            None => format!("trekr: {reason}"),
        });
    }
    match (
        value["counts"]["confirmed"].as_i64(),
        value["counts"]["possible"].as_i64(),
    ) {
        (Some(confirmed), Some(possible)) => Refs::Counts {
            confirmed,
            possible,
        },
        _ => Refs::Unavailable("trekr returned no counts".into()),
    }
}

/// Unix seconds as a plain date. No clock time: the signal is "which came
/// first by years", and an hour of precision would be decoration.
fn date(at: i64) -> String {
    // Civil-from-days, Howard Hinnant's algorithm. Chrono would be a
    // dependency for one function that has no timezone question in it — the
    // answer is a date in UTC either way.
    let days = at.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A duration in the largest unit that keeps it a small whole number.
///
/// Rounded down to that unit on purpose: "oldest by 3 years" is a claim the
/// measurement supports, where "oldest by 3.4 years" reads as though the
/// difference between two commit dates meant something to a decimal place.
fn span(seconds: i64) -> String {
    let days = seconds / 86_400;
    let plural = |n: i64, unit: &str| match n {
        1 => format!("1 {unit}"),
        n => format!("{n} {unit}s"),
    };
    match days {
        d if d >= 365 => plural(d / 365, "year"),
        d if d >= 30 => plural(d / 30, "month"),
        d if d >= 1 => plural(d, "day"),
        _ => "under a day".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(values: &[(&str, i64)]) -> Vec<Measured> {
        values
            .iter()
            .map(|(id, value)| Measured {
                id: (*id).into(),
                value: *value,
                display: value.to_string(),
            })
            .collect()
    }

    fn signal(name: &'static str, weight: &'static str, picks: Option<&str>) -> Signal {
        Signal {
            signal: name,
            weight,
            status: if picks.is_some() { "measured" } else { "tied" },
            picks: picks.map(str::to_string),
            note: format!("{name} said something"),
            measured: Vec::new(),
        }
    }

    #[test]
    fn a_tie_is_silence_rather_than_a_coin_flip() {
        assert_eq!(
            winner(&measured(&[("a", 1), ("b", 2)]), true).map(|m| m.id.as_str()),
            Some("a")
        );
        assert_eq!(
            winner(&measured(&[("a", 1), ("b", 2)]), false).map(|m| m.id.as_str()),
            Some("b")
        );
        assert!(winner(&measured(&[("a", 1), ("b", 1)]), true).is_none());
        // Three members where two share the extreme: still a tie, because the
        // question is which single member wins.
        assert!(winner(&measured(&[("a", 1), ("b", 1), ("c", 9)]), true).is_none());
    }

    #[test]
    fn agreeing_signals_pick_and_disagreeing_ones_abstain() {
        let agree = vec![
            signal("git_age", DECIDING, Some("A#run")),
            signal("references", DECIDING, Some("A#run")),
            signal("namespace_depth", TIEBREAK, Some("B#run")),
        ];
        assert_eq!(resolve(&agree).0, Some("A#run".into()));

        let disagree = vec![
            signal("git_age", DECIDING, Some("A#run")),
            signal("references", DECIDING, Some("B#run")),
        ];
        let (pick, basis) = resolve(&disagree);
        assert_eq!(pick, None, "a disagreement is reported, not resolved");
        assert!(basis.contains("disagree"), "got {basis:?}");
    }

    /// The weak signal decides only when both deciding ones are silent, and
    /// the sentence says the basis is weak.
    #[test]
    fn the_tiebreak_decides_only_in_silence() {
        let silent = vec![
            signal("git_age", DECIDING, None),
            unavailable("references", DECIDING, "trekr reads Ruby".into()),
            signal("namespace_depth", TIEBREAK, Some("A#run")),
        ];
        let (pick, basis) = resolve(&silent);
        assert_eq!(pick, Some("A#run".into()));
        assert!(basis.contains("weak"), "got {basis:?}");

        let nothing = vec![
            signal("git_age", DECIDING, None),
            signal("namespace_depth", TIEBREAK, None),
        ];
        assert_eq!(resolve(&nothing).0, None);
    }

    #[test]
    fn a_date_and_a_span_read_the_way_a_person_writes_them() {
        // `git log -1 --format=%at` on a known commit, cross-checked against
        // the date git prints for it.
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_351_209_600), "2012-10-26");
        assert_eq!(date(1_756_166_400), "2025-08-26");
        assert_eq!(span(0), "under a day");
        assert_eq!(span(86_400), "1 day");
        assert_eq!(span(86_400 * 45), "1 month");
        assert_eq!(span(86_400 * 400), "1 year");
        assert_eq!(span(86_400 * 1200), "3 years");
    }
}
