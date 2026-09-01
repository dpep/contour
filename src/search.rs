//! English queries, and nearest neighbours.
//!
//! Two halves, fused. The **lexical** half matches the query against a unit's
//! name; the **semantic** half matches it against the embedding of that unit's
//! summary. Reciprocal Rank Fusion merges them (gqls's `combine`, K=60): it is
//! scale-free, so a token-overlap count and a cosine need no common
//! normalization, and a unit strong in both rises above one strong in either.
//!
//! **RRF discards the score and keeps only the rank**, which is the property
//! that makes it scale-free and also the one both halves have had to correct
//! for. Each contributes `weight / (K + rank)`, and the weight is what says
//! how strong the evidence behind that rank was:
//!
//! - The semantic half is weighted by **which vector answered** (DEC-023): a
//!   summary is worth more than an identifier, because it is what the code does
//!   rather than a second look at the name the lexical half already read. A
//!   field trial on a partly-summarized rq had the summary that answered the
//!   question ranking fifth at cosine 0.44, under identifier hits at 0.20 whose
//!   long snake_case names shared query tokens by accident.
//! - The lexical half is weighted by **how much of the query the name accounted
//!   for** — [`lexical_score`], which it already computed and the fusion then
//!   threw away. Without it, sharing one filler word with the query bought the
//!   same place in the fusion as answering the query outright, and with K=60 it
//!   went on doing so a hundred places down the list.
//!
//! Neither weight is a knob. One names a tier; the other is a measurement the
//! ranking was already making.
//!
//! Every answer discloses what produced it (DEC-010) and how much of the
//! corpus could have participated (DEC-009). The semantic half only covers
//! summarized units, so a search over a half-summarized repo answers from what
//! exists and says so — rather than looking like the corpus is small.

use crate::embed::{Embedder, Prefer, config_key, humanize, mrl, tokenize};
use crate::store::{Located, Store};
use crate::summary::Coverage;
use anyhow::{Result, bail};
use std::collections::HashMap;

/// Reciprocal Rank Fusion constant, from gqls. Large enough that the top few
/// ranks are not winner-take-all.
const RRF_K: f64 = 60.0;

/// What a **summary** match is worth in the fusion, against a name match's 1.0.
///
/// Parity, and the reason is DEC-018's: a summary is what the code *does*,
/// bought with somebody's tokens and attention. Weighing it below a name match
/// was the flywheel quietly failing to pay off — see the module header for the
/// field trial that measured it.
const SUMMARY_WEIGHT: f64 = 1.0;

/// What an **identifier** match is worth: less, because it is a second look at
/// the same evidence the lexical half already used.
///
/// This is the constant that used to weigh the whole semantic half, and it
/// keeps its old value so a corpus with no summaries ranks exactly as it did.
/// The two weights differ only where the tiers meet, which is precisely where
/// the old single weight was wrong: it let a name-shaped match at cosine 0.20
/// outrank a summary that answered the question at 0.44.
const IDENTIFIER_WEIGHT: f64 = 0.7;

/// How much a hit outside the app population is discounted in the fusion.
///
/// The complaint this answers: on berater, "limit how many things can run at
/// once" returned `spec/riddle_spec.rb:13 limit` above `Berater::Limiter#limit`
/// — a spec defines a helper with the obvious name, and nothing told the ranker
/// that a caller almost never wants it. DEC-022 rules that the fix is a
/// **discount, not an exclusion**: test code is still an answer, and "why is my
/// spec not in the results" is a worse surprise than the one this fixes.
///
/// Not calibrated, and disclosed as `discount` on every answer, because DEC-011
/// says a ranking constant comes from the eval set and no labeled query expects
/// a test method either way. What it is measured against is **regression**: at
/// 0.5 no labeled query on any of the seven sets changes rank, and the two
/// known live cases flip. A reader who disagrees can see the number and the
/// `class` on each hit that it was applied to.
///
/// Fixtures take the same discount as tests. They are further still from what a
/// behavioural query is asking about, and inventing a second constant to say so
/// would be two numbers neither of which is measured.
pub const NON_APP_DISCOUNT: f64 = 0.5;

/// Cosine below which a hit is not an answer to anything — but only for an
/// embedder this was measured for.
///
/// Callers pass the floor in explicitly rather than having `search` read it,
/// so that `contour eval` can run with **no floor at all**. Calibrating a
/// threshold against results the threshold already filtered would only ever
/// show what survives today — a mistake the eval caught in its own harness.
///
/// gqls measured 0.40 on 256-dim MiniLM vectors over a 4602-record GraphQL
/// schema: the weakest real answer scored 0.526, the loudest nonsense 0.309.
/// That is **inherited, not calibrated for method summaries** — DEC-011 says
/// this number is settled by contour's own eval set, and until that exists it
/// is the best available guess rather than a measurement of this corpus.
///
/// The hash embedder gets **no floor at all**. It is lexical rather than
/// trained, so its cosine distribution is a different animal, and applying a
/// MiniLM-derived constant to it would be exactly the sort of borrowed number
/// that looks like evidence and is not. `CONTOUR_SEMANTIC_FLOOR` overrides
/// either way; `0` switches it off.
pub fn relevance_floor(kind: &str) -> f32 {
    if let Some(override_) = std::env::var("CONTOUR_SEMANTIC_FLOOR")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return override_;
    }
    match kind {
        "onnx" => 0.40,
        _ => 0.0,
    }
}

/// One search result.
#[derive(Debug, serde::Serialize)]
pub struct Hit {
    /// Absolute (`paths::absolute`).
    pub path: String,
    pub id: String,
    pub line: u32,
    /// Which half found it: `lexical`, `semantic`, or `both`.
    pub how: &'static str,
    /// Which vector the semantic half used, when it contributed:
    /// `summary` (what the code does) or `identifier` (what it is called).
    /// The distinction matters to a reader — an identifier-tier hit is blind
    /// to behaviour that never appears in a name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_via: Option<&'static str>,
    /// The semantic half's measurement, when it contributed. Absent when the
    /// unit was found by name alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f64>,
    /// The lexical half's measurement, when it contributed: how much of the
    /// query this unit's name accounts for, 0 to 1.
    ///
    /// Disclosed for the same reason `cosine` is (DEC-010). The lexical half
    /// used to be a predicate — a name either shared query words or did not —
    /// and `how: both` said everything there was to say. It is now graded, and
    /// `both` on a name that matched one filler word means something very
    /// different from `both` on a name that answered the whole query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical: Option<f64>,
    /// Why a container answered for this unit, when one did.
    ///
    /// Nomination is a third source of evidence and it does not belong in
    /// `how`, which says which *half* found a unit and would need five values
    /// to also say this. A reader shown `BackupService#call` for a query none
    /// of whose words are in it deserves to be told that its class was what
    /// matched, and which rule let the class speak for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominated: Option<Nomination>,
    /// The fused rank score, after any path-class discount. Deliberately
    /// **not** called confidence: RRF is scale-free and its value means nothing
    /// outside this result set (DEC-010). `cosine` above is the measurement a
    /// reader should weigh.
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What kind of file this hit lives in (DEC-022). `test` or `fixture` here
    /// is also the reason `score` carries the [`NON_APP_DISCOUNT`] the answer
    /// discloses — the tag and the ranking treatment are the same fact.
    pub class: crate::paths::Class,
}

/// A container that answered for one of its units, and how well it matched.
///
/// `rule` is a sentence rather than a code because there is exactly one rule
/// and a reader should not have to look it up (DEC-028). If a second one is
/// ever earned, this is where it says which fired.
#[derive(Debug, serde::Serialize)]
pub struct Nomination {
    /// The container's lexical owner, as a reader would write it.
    pub container: String,
    pub rule: &'static str,
    /// The query against the container's centroid — the running mean of its
    /// members' vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f64>,
}

impl Nomination {
    fn of(container: &Container) -> Nomination {
        Nomination {
            container: container.owner.clone(),
            rule: "its container's only public unit",
            cosine: None,
        }
    }
}

/// A search answer plus everything needed to judge it.
#[derive(Debug, serde::Serialize)]
pub struct Answer {
    /// The checkout this answer is about. Record paths are absolute, so this
    /// is not needed to resolve them — it names what was searched.
    pub root: String,
    pub hits: Vec<Hit>,
    /// `complete` / `warming` / `none`, with the counts behind it.
    pub coverage: Coverage,
    pub coverage_state: &'static str,
    /// Which embedder answered — `hash` means the semantic half is lexical
    /// feature hashing, not a trained model, and should be read accordingly.
    pub embedder: &'static str,
    /// How many ranked units were reached through each vector tier. An answer
    /// drawn mostly from identifiers is a weaker answer than one drawn from
    /// summaries, and a reader cannot tell from the hits alone.
    pub tiers: Tiers,
    /// The absolute cosine floor applied, so a reader knows whether "no
    /// matches" means "nothing above the bar" or "no bar was set".
    pub floor: f64,
    /// Units the floor removed that would otherwise have ranked.
    ///
    /// The floor is inherited from another corpus and contour's own eval
    /// argues with it, so shipping it silently would be shipping an
    /// unfalsifiable constant. Reporting what it hid makes it auditable —
    /// trekr's `--include-excluded` move — and `--floor 0` shows the rest.
    pub withheld: usize,
    /// Ranked units the path policy removed, by class (DEC-022). The same
    /// argument as `withheld` above, applied to the other default that hides
    /// results.
    pub withheld_paths: crate::paths::Withheld,
    /// The discount applied to hits outside the app population, so a reader
    /// can see the ranking knob rather than infer it. `1.0` means none was.
    pub discount: f64,
}

/// How many units each vector tier covered.
#[derive(Debug, Default, serde::Serialize)]
pub struct Tiers {
    pub summary: usize,
    pub identifier: usize,
}

/// Everything about a query except the query.
///
/// A struct rather than five more parameters: the call sites read as English
/// this way, and the next option to arrive does not grow a signature nobody
/// can hold in their head.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    /// Repo-relative path prefix, or `None` for the whole checkout.
    pub scope: Option<&'a str>,
    pub limit: usize,
    /// Cosine floor. `0.0` withholds nothing — which is what the eval uses,
    /// because calibrating a threshold above itself proves nothing.
    pub floor: f32,
    pub prefer: Prefer,
    /// Which path classes are answers, and which are ranked apart (DEC-022).
    /// Borrowed, because it is one value per command and every query in a run
    /// must agree about it.
    pub classes: &'a crate::paths::Classes,
}

impl<'a> Options<'a> {
    /// Everything at its default, against a given path policy. There is no
    /// `Default`: a policy is loaded from the checkout, and defaulting it here
    /// would let a caller search one repo under another's rules.
    pub fn new(classes: &'a crate::paths::Classes) -> Options<'a> {
        Options {
            scope: None,
            limit: 10,
            floor: 0.0,
            prefer: Prefer::Best,
            classes,
        }
    }
}

/// Rank units in a checkout against an English query.
pub fn search(
    store: &mut Store,
    root: &str,
    query: &str,
    embedder: &dyn Embedder,
    options: Options<'_>,
) -> Result<Answer> {
    let Options {
        scope,
        limit,
        floor,
        prefer,
        classes,
    } = options;
    let units = in_scope(store, root, scope)?;
    // Per unit, not per path: Rust's tests live inside the file they test, and
    // the discount has to reach them (see `Classes::of_unit`).
    let class_of: Vec<crate::paths::Class> = units
        .iter()
        .map(|u| classes.of_unit(&u.path, &u.unit))
        .collect();
    let summaries = vectors_for(store, &units, embedder, prefer)?;

    // Lexical: how much of the query each humanized name accounts for.
    let mut lexical: Vec<(usize, f64)> = units
        .iter()
        .enumerate()
        .map(|(i, u)| (i, lexical_score(query, &u.unit.id())))
        .filter(|(_, score)| *score > 0.0)
        .collect();
    lexical.sort_by(|a, b| b.1.total_cmp(&a.1));

    // Semantic: cosine against each summary's vector, floored.
    let query_vec = mrl::compress_matryoshka_vector(&embedder.embed(query));
    let scored: Vec<(usize, f32)> = summaries
        .vectors
        .iter()
        .map(|(i, (vec, _))| (*i, mrl::cosine_similarity(&query_vec, vec)))
        .filter(|(_, cosine)| *cosine > 0.0)
        .collect();
    let mut semantic: Vec<(usize, f32)> = scored
        .iter()
        .copied()
        .filter(|(_, cosine)| *cosine >= floor)
        .collect();
    let withheld = scored.len() - semantic.len();
    // Ties broken by unit index, because RRF consumes a *rank*: the list is
    // built by walking a HashMap, so two units with the same cosine — the same
    // name in two files is enough — were handed ranks in whatever order the map
    // yielded, and that changes the fused score. Two runs of one query
    // disagreed about their order, which is the property the sort below
    // already claims to guarantee.
    semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    // Containers: the same two halves over a coarser candidate, each answered
    // for by the container's nominee. Ranked separately rather than mixed into
    // the lists above, so a unit index never has to mean two things.
    let containers = containers(&units, &summaries.vectors);
    let mut container_semantic: Vec<(usize, f32)> = containers
        .iter()
        .enumerate()
        .filter_map(|(c, container)| {
            let centroid = container.centroid.as_ref()?;
            Some((c, mrl::cosine_similarity(&query_vec, centroid)))
        })
        .filter(|(_, cosine)| *cosine >= floor.max(f32::MIN_POSITIVE))
        .collect();
    container_semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let cosines: HashMap<usize, f32> = semantic.iter().copied().collect();
    let scores: HashMap<usize, f64> = lexical.iter().copied().collect();
    let mut fused: HashMap<usize, (f64, bool, bool)> = HashMap::new();
    for (rank, (i, score)) in lexical.iter().enumerate() {
        // Weighted by how much of the query the name accounted for — the same
        // argument DEC-023 made for the semantic half, and for the same reason.
        // RRF consumes a rank and throws the score away, so a name that
        // answered the whole query and one that shared a single filler word are
        // worth the same, and with K=60 that stays true a hundred places down
        // the list. A full match still scores exactly 1.0, so the top of the
        // lexical ranking is untouched and only weak evidence is priced as
        // weak. Not a constant: the weight is the measurement.
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += score / (RRF_K + rank as f64 + 1.0);
        entry.1 = true;
    }
    for (rank, (i, _)) in semantic.iter().enumerate() {
        // Weighted by which vector answered. RRF consumes a rank and throws the
        // cosine away, so without this a summary hit and an identifier hit two
        // places above it are worth almost the same — and the identifier hit
        // wins on a long snake_case name that happens to share query tokens.
        let weight = match summaries.vectors.get(i).map(|(_, via)| *via) {
            Some("summary") => SUMMARY_WEIGHT,
            _ => IDENTIFIER_WEIGHT,
        };
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += weight / (RRF_K + rank as f64 + 1.0);
        entry.2 = true;
    }

    // A container's rank goes to the unit it nominates, weighted by
    // `cosine * IDENTIFIER_WEIGHT` — two multiplications for two facts, and
    // neither is a new constant.
    //
    // `IDENTIFIER_WEIGHT` is the floor DEC-023 already defines, taken even
    // where every member is summarized: a centroid is the mean of vectors none
    // of which is the unit being nominated, and DEC-013 says routing is a
    // ranking bias, never a prune.
    //
    // The **cosine** is there for DEC-027's reason. At the unit level a tier
    // says what kind of evidence answered and the rank carries the rest; here
    // every centroid is the same kind of evidence, so the cosine is the only
    // thing distinguishing one container's claim from another's, and the rank
    // alone conveys almost nothing. Measured: at a flat weight this cost three
    // top-1s across the Rust sets, because a module whose one `pub fn` is
    // unrelated was lifted as hard as a service class that answered. With the
    // measurement in the weight, top-1 holds and top-5 gains.
    let mut nominated: HashMap<usize, Nomination> = HashMap::new();
    for (rank, (c, cosine)) in container_semantic.iter().enumerate() {
        let entry = fused
            .entry(containers[*c].nominee)
            .or_insert((0.0, false, false));
        entry.0 += (*cosine as f64) * IDENTIFIER_WEIGHT / (RRF_K + rank as f64 + 1.0);
        nominated
            .entry(containers[*c].nominee)
            .or_insert_with(|| Nomination::of(&containers[*c]))
            .cosine = Some(round2(*cosine));
    }

    // The path policy, applied where ranking is decided: an ignored class is
    // not an answer, and a class ranked apart is discounted rather than
    // dropped (DEC-022). Both happen before the truncation, so neither can
    // spend a slot a reader wanted.
    let mut withheld_paths = crate::paths::Withheld::default();
    let mut ranked: Vec<(usize, f64, bool, bool)> = fused
        .into_iter()
        .filter_map(|(i, (score, lex, sem))| {
            if classes.hides(&units[i].path, &mut withheld_paths) {
                return None;
            }
            let discount = match class_of[i].is_app() {
                true => 1.0,
                false => NON_APP_DISCOUNT,
            };
            Some((i, score * discount, lex, sem))
        })
        .collect();
    // Ties broken by location, so two runs of the same query agree.
    ranked.sort_by(|a, b| {
        b.1.total_cmp(&a.1).then_with(|| {
            (&units[a.0].path, units[a.0].unit.line).cmp(&(&units[b.0].path, units[b.0].unit.line))
        })
    });
    ranked.truncate(limit);

    let hits = ranked
        .into_iter()
        .map(|(i, score, lex, sem)| Hit {
            class: class_of[i],
            path: crate::paths::absolute(root, &units[i].path),
            id: units[i].unit.id(),
            line: units[i].unit.line,
            how: match (lex, sem) {
                (true, true) => "both",
                (false, true) => "semantic",
                _ => "lexical",
            },
            cosine: cosines.get(&i).copied().map(round2),
            lexical: scores.get(&i).copied().map(|score| round2(score as f32)),
            nominated: nominated.remove(&i),
            semantic_via: match cosines.contains_key(&i) {
                true => summaries.vectors.get(&i).map(|(_, via)| *via),
                false => None,
            },
            // An RRF score is a rank artefact, and 0.02768376520359598 claims
            // sixteen digits of evidence that a fusion of two rankings over a
            // few hundred units does not have. Rounded here rather than in the
            // renderer, so the JSON cannot keep digits the human output hid.
            score: (score * 10_000.0).round() / 10_000.0,
            summary: summaries.text.get(&i).cloned(),
        })
        .collect();

    let mut tiers = Tiers::default();
    for (_, via) in summaries.vectors.values() {
        match *via {
            "summary" => tiers.summary += 1,
            _ => tiers.identifier += 1,
        }
    }

    Ok(Answer {
        root: root.to_string(),
        hits,
        tiers,
        coverage_state: summaries.coverage.state(),
        coverage: summaries.coverage,
        embedder: embedder.kind(),
        floor: round2(floor),
        withheld,
        withheld_paths,
        discount: NON_APP_DISCOUNT,
    })
}

/// Two decimals: a cosine off a 256-dim vector has about that much meaning,
/// and a floor of 0.4000000059604645 is an f32 artefact rather than a threshold
/// anybody chose. Rounded where the value is built (the house rule), so JSON
/// and human output cannot disagree about how much precision exists.
///
/// **Out as f64, and that is the whole point.** Rounding an f32 leaves an f32,
/// and serialization widens it back to a double — so `0.45f32` reached every
/// agent as `0.44999998807907104`, digits the human output had been careful to
/// hide. The rounding has to survive the type it is serialized through.
pub(crate) fn round2(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}

/// One neighbour of a named unit, and how it was found.
#[derive(Debug, serde::Serialize)]
pub struct Neighbor {
    /// Absolute (`paths::absolute`).
    pub path: String,
    pub id: String,
    pub line: u32,
    /// The tier this was found by:
    ///
    /// - `structural` — an identical normalized body. A predicate, so it
    ///   carries evidence (`lines`) rather than a confidence (DEC-010).
    /// - `semantic` — a nearby summary embedding, with the cosine as its
    ///   confidence, because there the judgment really is graded.
    ///
    /// - `near_structural` — a nearly identical shape, with the Jaccard over
    ///   subtree signatures (`crate::near`). Also graded, also reported as the
    ///   measurement.
    pub how: &'static str,
    /// The semantic tier's cosine. Named the same as `Hit::cosine`, because it
    /// is the same measurement: one field called `confidence` here and
    /// `cosine` there made a consumer handle two names for one number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f64>,
    /// The near tier's Jaccard, named the same as `dupes::Group::similarity`
    /// for the same reason. Two different measurements sharing one field was
    /// the other half of the drift.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What kind of file this neighbour lives in (DEC-022). Unlike `search`,
    /// nothing here is discounted for it: a neighbour list is short and its
    /// order is the tier's measurement, so the tag is the whole treatment.
    pub class: crate::paths::Class,
}

/// Nearest neighbours of one named unit, structural tier first.
///
/// The unit asked about is resolved against the whole checkout, ignored paths
/// included — you can ask about a migration you are looking at, and about one
/// that sits outside the `scope` its neighbours are sought in. Its
/// *neighbours* follow the path policy, and say what they withheld.
///
/// **`scope` is what makes this affordable on a large corpus.** Every unit in
/// it needs a vector, and one that has none is embedded on the spot — so an
/// unscoped call on a cold monorepo is an embedding run, not a query.
pub fn similar(
    store: &mut Store,
    root: &str,
    id: &str,
    scope: Option<&str>,
    embedder: &dyn Embedder,
    limit: usize,
    classes: &crate::paths::Classes,
) -> Result<Neighbors> {
    let mut units = in_scope(store, root, None)?;
    let mut target = resolve(&units, id)?;
    // Narrowed after resolution rather than before it, so a scope that
    // excludes the unit asked about is a smaller search and not a "no such
    // unit" — the target is the query, never one of the answers.
    if let Some(scope) = scope {
        let mut kept = Vec::new();
        for (i, unit) in units.into_iter().enumerate() {
            if i == target || crate::paths::under(&unit.path, scope) {
                if i == target {
                    target = kept.len();
                }
                kept.push(unit);
            }
        }
        units = kept;
    }
    let summaries = vectors_for(store, &units, embedder, Prefer::Best)?;
    let here = &units[target];
    let floor = relevance_floor(embedder.kind());
    let mut withheld = 0;
    let mut withheld_paths = crate::paths::Withheld::default();

    // Which units a tier has already reported. Keyed by **index**, not by the
    // path string: `path` is absolute on a `Neighbor` and relative on a
    // `Located`, so comparing them silently never matched, and the same unit
    // came back twice — once `structural`, once `semantic cos 1.00`. Reported
    // by QA against rails and reintroduced here the moment paths changed
    // shape, which is the argument for keying on identity rather than on
    // whatever the renderer happens to be doing.
    let mut claimed: std::collections::HashSet<usize> = std::iter::once(target).collect();

    // Structural: an identical normalized body, at a different place.
    let mut out: Vec<Neighbor> = Vec::new();
    if let Some(norm_hash) = here.unit.norm_hash {
        for (i, other) in units.iter().enumerate() {
            let same_span = other.path == here.path && other.unit.line == here.unit.line;
            if i == target || same_span || other.unit.norm_hash != Some(norm_hash) {
                continue;
            }
            claimed.insert(i);
            // Each tier drops its own, rather than one filter over the merged
            // list: the semantic tier truncates to `limit` on the way out, so a
            // late filter would spend slots on answers nobody sees.
            if classes.hides(&other.path, &mut withheld_paths) {
                continue;
            }
            out.push(Neighbor {
                path: crate::paths::absolute(root, &other.path),
                id: other.unit.id(),
                line: other.unit.line,
                how: "structural",
                cosine: None,
                similarity: None,
                lines: Some(other.unit.end_line + 1 - other.unit.line),
                summary: summaries.text.get(&i).cloned(),
                class: classes.of_unit(&other.path, &other.unit),
            });
        }
    }

    // Near-structural: mostly the same shape, between exact identity and
    // meaning. Runs before the semantic tier so a reader sees the cheaper,
    // sharper evidence first.
    {
        for near in crate::near::neighbors(store, root, here, crate::near::NEAR_THRESHOLD, scope)? {
            let index = units
                .iter()
                .position(|u| u.path == near.path && u.unit.line == near.line);
            if index.is_some_and(|i| !claimed.insert(i)) {
                continue;
            }
            if classes.hides(&near.path, &mut withheld_paths) {
                continue;
            }
            out.push(Neighbor {
                // The near tier reports a location rather than a unit, so
                // the unit-aware class needs the index that located it.
                // Falling back to the path is not a second rule: `index` is
                // `None` only for a neighbour this checkout holds no unit for,
                // which is also the only case with no unit to ask.
                class: match index {
                    Some(i) => classes.of_unit(&units[i].path, &units[i].unit),
                    None => classes.of(&near.path),
                },
                path: crate::paths::absolute(root, &near.path),
                id: near.id,
                line: near.line,
                how: "near_structural",
                cosine: None,
                similarity: Some(round2(near.similarity)),
                lines: Some(near.end_line + 1 - near.line),
                summary: index.and_then(|i| summaries.text.get(&i).cloned()),
            });
        }
    }

    // Semantic: nearest summaries, minus anything a structural tier already
    // claimed — a result reported twice under two tiers is a result a reader
    // has to de-duplicate by hand.
    if let Some((vec, _)) = summaries.vectors.get(&target) {
        let mut scored: Vec<(usize, f32)> = summaries
            .vectors
            .iter()
            .filter(|(i, _)| !claimed.contains(*i))
            .map(|(i, (other, _))| (*i, mrl::cosine_similarity(vec, other)))
            .collect();
        // Counted before it is applied, so "nothing similar" and "nothing
        // above the bar" are never the same silence (DEC-010).
        withheld = scored.iter().filter(|(_, c)| *c < floor).count();
        scored.retain(|(_, cosine)| *cosine >= floor);
        scored.retain(|(i, _)| !classes.hides(&units[*i].path, &mut withheld_paths));
        // Ties by unit index, for the reason `search` above spells out: this
        // list is walked out of a HashMap, and `take(limit)` below turns a tie
        // into a coin flip about which neighbour is reported at all.
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (i, cosine) in scored.into_iter().take(limit) {
            out.push(Neighbor {
                class: classes.of_unit(&units[i].path, &units[i].unit),
                path: crate::paths::absolute(root, &units[i].path),
                id: units[i].unit.id(),
                line: units[i].unit.line,
                how: "semantic",
                // A graded judgment, so it gets a number — and the number is
                // the measurement itself, not a mapping of it.
                cosine: Some(round2(cosine)),
                similarity: None,
                lines: None,
                summary: summaries.text.get(&i).cloned(),
            });
        }
    }
    out.truncate(limit);
    Ok(Neighbors {
        root: root.to_string(),
        scope: scope.map(str::to_string),
        unit: here.unit.id(),
        path: crate::paths::absolute(root, &here.path),
        line: here.unit.line,
        neighbors: out,
        coverage_state: summaries.coverage.state(),
        coverage: summaries.coverage,
        embedder: embedder.kind(),
        floor: round2(floor),
        withheld,
        withheld_paths,
    })
}

/// Neighbours of one unit, with what the search could and could not see.
///
/// An object rather than a bare list, for the same reason `search` returns
/// one: a reader shown three neighbours has no way to tell a thin corpus from
/// a thorough answer, and `similar` used to disclose nothing at all — no
/// coverage, no floor, no count of what the floor withheld (DEC-010). An empty
/// result was zero bytes and an exit code.
#[derive(Debug, serde::Serialize)]
pub struct Neighbors {
    pub root: String,
    /// The path prefix the neighbours were sought under, absent for the whole
    /// checkout. Disclosed because a caller standing in a subdirectory gets
    /// one without asking, and three neighbours from `app/billing` read
    /// exactly like three from a thin corpus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// The unit that was asked about, resolved — which matters when the name
    /// given was ambiguous and a location settled it.
    pub unit: String,
    pub path: String,
    pub line: u32,
    pub neighbors: Vec<Neighbor>,
    pub coverage: Coverage,
    pub coverage_state: &'static str,
    pub embedder: &'static str,
    /// The cosine floor applied to the semantic tier. The structural and
    /// near-structural tiers are predicates and no floor touches them.
    pub floor: f64,
    pub withheld: usize,
    /// Neighbours the path policy removed, by class — from every tier, since
    /// an identical body in `vendor/` is as much a non-answer as a nearby one
    /// (DEC-022).
    pub withheld_paths: crate::paths::Withheld,
}

/// Resolve what the user typed to exactly one unit.
///
/// Accepts the id every surface prints (`Owner#method`, `Owner::fn`) and, when
/// that is not unique, the `path:line` every surface prints beside it.
///
/// **An ambiguous name is refused, not resolved.** rails' two
/// `ConnectionPool::Wrapper#method_missing` defs are the worked example: with
/// the name alone contour picked whichever the index stored first and then
/// listed the other one, printing the same id twice, so the answer was
/// unreadable — the reader could not tell the query from the result. DEC-010
/// gives ambiguity its own status rather than a quiet resolution, and this is
/// the same medicine a canonical pick needed: identify a unit by where it is,
/// not only by what it is called.
fn resolve(units: &[Located], target: &str) -> Result<usize> {
    // `path:line` first: it is unambiguous by construction, and a filename
    // cannot collide with a unit id because ids carry no colon-then-digits.
    if let Some((path, line)) = target.rsplit_once(':')
        && let Ok(line) = line.parse::<u32>()
    {
        return units
            .iter()
            .position(|u| u.path == path && u.unit.line == line)
            .ok_or_else(|| anyhow::anyhow!("no unit at {target} in this checkout"));
    }

    let matches: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.unit.id() == target)
        .map(|(i, _)| i)
        .collect();
    match matches.as_slice() {
        [] => match nearest(units, target) {
            // A typo is the likeliest reason a name misses, and the index
            // already holds every alternative. Offering three costs one pass.
            Some(near) => bail!("no unit named `{target}` in this checkout. Did you mean:\n{near}"),
            None => bail!("no unit named `{target}` in this checkout"),
        },
        [only] => Ok(*only),
        many => {
            // Bare locations, not `contour similar …`: this message is shared
            // with the MCP tool, where CLI syntax is the wrong instruction and
            // `path:line` is what both surfaces accept as the unit itself.
            let listed: Vec<String> = many
                .iter()
                .map(|i| format!("  {}:{}", units[*i].path, units[*i].unit.line))
                .collect();
            bail!(
                "`{target}` names {} units in this checkout; ask for one by location:\n{}",
                many.len(),
                listed.join("\n")
            )
        }
    }
}

/// The closest few ids to a name that missed.
///
/// Scored by how much of the typed name the candidate contains, in order —
/// enough to catch a transposition or a dropped letter, and deliberately not a
/// full edit distance, which would be a second ranking to calibrate for a
/// message nobody reads twice.
fn nearest(units: &[Located], target: &str) -> Option<String> {
    let wanted = target.to_lowercase();
    let mut scored: Vec<(usize, String)> = units
        .iter()
        .map(|u| u.unit.id())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            let overlap = common_prefix(&wanted, &id.to_lowercase());
            // Three quarters of the typed name has to match. At a half, every
            // sibling in the same class qualified on the shared owner alone,
            // and `method_mising` suggested `other`.
            (overlap * 4 >= wanted.len() * 3).then_some((overlap, id))
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Some(
        scored
            .iter()
            .take(3)
            .map(|(_, id)| format!("  {id}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// How many leading bytes two names share.
fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// How much of a query a unit's humanized name accounts for, 0 to 1.
///
/// Deliberately simple. RRF consumes a *ranking*, not a calibrated score, so
/// the elaborate fuzzy scorer gqls uses for GraphQL paths would be a borrowed
/// abstraction doing a job this does in twenty lines.
///
/// **The denominator is the whole query, findable or not.** A name that
/// accounted for one word of seven accounted for a seventh, whether or not the
/// other six appear anywhere in the corpus. Normalizing by what was reachable
/// instead was tried and measured (see `docs/PLAN.md`): it turns a seven-word
/// question into a two-word one and hands a name that matched only `a` half
/// the credit.
fn lexical_score(query: &str, id: &str) -> f64 {
    let name: Vec<String> = tokenize(&humanize(id)).collect();
    if name.is_empty() {
        return 0.0;
    }
    let query: Vec<String> = tokenize(query).collect();
    if query.is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    for term in &query {
        if name.iter().any(|w| w == term) {
            score += 1.0;
        } else if name.iter().any(|w| w.starts_with(term.as_str())) {
            // A prefix is real evidence but weaker: `pay` matching `payroll`
            // should not outrank `payroll` matching `payroll`.
            score += 0.5;
        }
    }
    score / query.len() as f64
}

/// A container, ranked as a candidate and answered for by one of its units.
///
/// **The finding this exists for** (M12b's mastodon census): an entry point is
/// named for the *protocol* it implements — `call`, `to_s`, `get`, `use`,
/// `hydrate`, `refresh` — and its private helpers are named for the
/// *behaviour*. So the class out-ranks its own entry point on 6 of 21 labeled
/// queries: `BackupService#call` sits at rank 40 while `#build_archive!` is
/// first, and `StatusCacheHydrator#hydrate` does not rank at all while
/// `#fill_status_payload` is first. The meaning is in the container; the thing
/// a caller wants is the one part of it that carries none.
///
/// So the container is ranked, and when it ranks it **nominates**. DEC-013's
/// thesis (coarser units, not blurrier ones) arriving early, and deliberately
/// **not** its record model: a container here is a query-time view over the
/// units already in hand — nothing stored, no cache key, no reindex, and the
/// answer is still a list of units, which is the one noun contour has.
///
/// ### The nomination rule: a container's sole public unit
///
/// One rule, no framework knowledge, and it abstains rather than guessing. A
/// container with exactly one public unit *is* that unit as far as a caller is
/// concerned — which is the service-object shape stated structurally instead of
/// by knowing what Rails calls a service. Nine of twenty labeled mastodon
/// answers are their container's sole public method, and they are the nine
/// broken cases.
///
/// The rule also bounds its own blast radius, which is why it needs no
/// threshold: a large class matches many queries and has many public methods,
/// so it never nominates. `FeedManager` has twenty and stays silent.
///
/// Two units minimum, or a container would be a second vote for a unit that is
/// already being ranked on its own name and its own vector.
struct Container {
    /// Unit index of the sole public unit — what this answers for.
    nominee: usize,
    /// Running mean of the members' vectors (ae's trick, DEC-013's cheap
    /// tier). `None` where no member has one.
    centroid: Option<Vec<f32>>,
    owner: String,
}

/// The containers in a set of units, keyed by lexical owner.
fn containers(
    units: &[Located],
    vectors: &HashMap<usize, (Vec<f32>, &'static str)>,
) -> Vec<Container> {
    let mut by_owner: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, located) in units.iter().enumerate() {
        if !located.unit.owner.is_empty() {
            by_owner.entry(&located.unit.owner).or_default().push(i);
        }
    }
    let mut out: Vec<Container> = by_owner
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .filter_map(|(owner, members)| {
            let mut public = members.iter().filter(|i| {
                let unit = &units[**i].unit;
                // `via.is_some()` is a macro-generated accessor — declared,
                // not written, and with no body to be an entry point of.
                // Rails classes carry `attr_reader` routinely, and without this
                // `BackupService` had three public units and nominated nobody.
                unit.visibility == crate::core::Visibility::Public && unit.via.is_none()
            });
            let nominee = *public.next()?;
            // Exactly one, or this container has no single front door and says
            // so by not answering (DEC-010's ambiguity-is-its-own-status).
            if public.next().is_some() {
                return None;
            }
            Some(Container {
                nominee,
                centroid: centroid(&members, vectors),
                owner: owner.to_string(),
            })
        })
        .collect();
    // Walked out of a HashMap, so ordered here: RRF consumes a rank, and a
    // tie handed out in map order makes one query's answer depend on hashing.
    out.sort_by_key(|c| c.nominee);
    out
}

/// The mean of whatever vectors a container's members have.
///
/// Not weighted by anything: a container's meaning is its parts, and a part
/// with a summary is already worth more because its vector is a better one.
fn centroid(
    members: &[usize],
    vectors: &HashMap<usize, (Vec<f32>, &'static str)>,
) -> Option<Vec<f32>> {
    let mut sum: Vec<f32> = Vec::new();
    let mut n = 0.0f32;
    for i in members {
        let Some((vec, _)) = vectors.get(i) else {
            continue;
        };
        if sum.is_empty() {
            sum = vec.clone();
        } else {
            for (acc, x) in sum.iter_mut().zip(vec) {
                *acc += x;
            }
        }
        n += 1.0;
    }
    if sum.is_empty() {
        return None;
    }
    for x in sum.iter_mut() {
        *x /= n;
    }
    Some(sum)
}

fn in_scope(store: &Store, root: &str, scope: Option<&str>) -> Result<Vec<Located>> {
    Ok(store
        .units(root)?
        .into_iter()
        .filter(|l| scope.is_none_or(|s| crate::paths::under(&l.path, s)))
        .collect())
}

/// The summaries and vectors for a set of units, embedding whatever is missing.
///
/// Embedding lazily here rather than in a command of its own keeps vectors in
/// step with summaries by construction: switching embedders re-embeds on the
/// next query instead of needing anyone to remember a step.
struct Indexed {
    /// Unit index → its vector and which tier produced it.
    vectors: HashMap<usize, (Vec<f32>, &'static str)>,
    /// Unit index → the human summary line, for display.
    text: HashMap<usize, String>,
    coverage: Coverage,
}

fn vectors_for(
    store: &mut Store,
    units: &[Located],
    embedder: &dyn Embedder,
    prefer: Prefer,
) -> Result<Indexed> {
    let model = embedder.model().to_string();
    let config = config_key(embedder.kind(), &model);
    let stored = store.all_summaries()?;

    let mut indexed = Indexed {
        vectors: HashMap::new(),
        text: HashMap::new(),
        coverage: Coverage::default(),
    };

    // A summary is only possible where there is a body to summarize; the
    // identifier tier covers everything, including the macro-generated units
    // that have no body at all.
    let summary_of = |located: &Located| {
        located.unit.norm_hash.and_then(|norm_hash| {
            let ctx = crate::summary::Context::of(&located.unit).hash();
            stored.get(&(norm_hash, ctx))
        })
    };

    // What every unit in scope is embedded as, worked out before a single
    // vector is read. That order is what makes the read scope-sized: the
    // vector table is keyed by text, so the texts *are* the scope, and asking
    // for them by name is the difference between loading a directory's
    // vectors and loading a monorepo's (DEC-033).
    //
    // The key is kept and the text is dropped. Holding a quarter of a million
    // live short strings to hand a fraction of them to the embedder costs
    // about 0.2 s of allocation on a large checkout, and the warm path — the
    // one that matters once `contour embed` has run — never wants them.
    let mut planned: Vec<(usize, u64, &'static str)> = Vec::with_capacity(units.len());
    for (i, located) in units.iter().enumerate() {
        let summary = summary_of(located);
        if located.unit.norm_hash.is_some() {
            indexed.coverage.summarizable += 1;
        }
        if let Some(summary) = summary {
            indexed.coverage.summarized += 1;
            indexed.text.insert(i, summary.summary.clone());
        }
        if let Some(text) = crate::embed::text_of(&located.unit, summary, prefer) {
            planned.push((i, text.key, text.via));
        }
    }

    let have = store.vectors(config, &planned.iter().map(|(_, key, _)| *key).collect())?;
    let mut missing: Vec<(u64, String)> = Vec::new();
    let mut pending: Vec<(usize, u64, &'static str)> = Vec::new();
    for (i, key, via) in planned {
        match have.get(&key) {
            Some(vec) => {
                indexed.vectors.insert(i, (vec.clone(), via));
            }
            None => {
                // Rebuilt rather than kept, per the note above. Only what has
                // no vector pays for it, and it is about to be embedded.
                if let Some(text) =
                    crate::embed::text_of(&units[i].unit, summary_of(&units[i]), prefer)
                {
                    missing.push((key, text.text));
                }
                pending.push((i, key, via));
            }
        }
    }

    if !missing.is_empty() {
        // A cold corpus is a long wait, and a silent one reads as a hang.
        //
        // Measured, and worth reading before changing anything here.
        //
        // Release, ONNX, 8 cores, contour's own 529 units:
        //
        // | path              | wall  | cpu   |
        // |-------------------|-------|-------|
        // | serial            |  3.7s | 14.5s |
        // | pooled (this one) |  3.9s | 16.1s |
        // | warm (cached)     | 0.18s |  0.2s |
        //
        // A wash at this size, and expected to be: each worker thread loads
        // its own ONNX session, so on a few hundred units that fixed cost IS
        // the run. The pool is meant to pay at corpus scale.
        //
        // **Measured at corpus scale, which the earlier retraction owed.**
        // rails, 54,296 texts, same build, same machine:
        //
        // | path                          | wall   | cpu     |
        // |-------------------------------|--------|---------|
        // | pooled (this one)             |   167s |   1192s |
        // | one worker (RAYON_NUM_THREADS=1) | >600s | —      |
        // | warm (cached vectors)         |   5.4s |    0.4s |
        //
        // The one-worker run was abandoned at a ten-minute timeout, so the
        // pool is **at least 3.6x** and the true figure is larger. 1192s of
        // cpu against 167s of wall is 7.1x on 8 cores, which says the
        // parallelism is real rather than nominal. The pool is worth keeping.
        //
        // Conditions, because they change how much to trust this: load
        // average ~3 on 8 cores at the start of the pooled run, so the wall
        // figure is if anything pessimistic. The earlier 73s/120s figures for
        // rails are withdrawn rather than compared against — they were taken
        // on the serial path while `embed_all` was dead code, and nothing
        // about them is reproducible now.
        //
        // Note what `warm` costs at this size: **5.4s, not the 0.18s measured
        // on contour's own 529 units.** Loading and scoring 54k cached vectors
        // is most of it. "Instant after the first query" is true of a small
        // repo and an overstatement of a large one.
        //
        // If this needs to be faster, the lever is embedding *less* — a
        // scope-bounded warm — rather than restructuring this loop: `user`
        // running ~4x `wall` says the parallelism is already real.
        // One vector per distinct text: identical summaries embed once. Deduped
        // before the estimate below, because it is the distinct texts that get
        // embedded and quoting the other number would overstate the bill.
        let mut unique: Vec<(u64, String)> = missing;
        unique.sort_unstable_by_key(|(key, _)| *key);
        unique.dedup_by_key(|(key, _)| *key);
        let texts: Vec<String> = unique.iter().map(|(_, text)| text.clone()).collect();

        crate::embed::afford(embedder.kind(), texts.len(), units.len())?;
        if texts.len() > 2_000 {
            eprintln!(
                "contour: embedding {} texts with the {} embedder — this is a one-time \
                 cost per corpus and is cached; subsequent queries are instant",
                texts.len(),
                embedder.kind()
            );
        }

        // `None`: no `--embed-model` flag exists yet, so the spec is genuinely
        // constant and the pool resolves the same embedder `embedder` did —
        // which it must, because `config` above was keyed from that one.
        let vectors = crate::embed::embed_all(None, &texts);
        // Before anything is written: a cancelled run comes back holding empty
        // vectors for whatever it did not reach, and storing those would poison
        // the cache with answers no embedder produced.
        crate::cancel::current().check()?;
        let fresh: HashMap<u64, Vec<f32>> = unique
            .into_iter()
            .map(|(key, _)| key)
            .zip(vectors)
            .collect();
        store.put_vectors(
            config,
            &fresh
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect::<Vec<_>>(),
        )?;
        for (i, key, via) in pending {
            if let Some(vec) = fresh.get(&key) {
                indexed.vectors.insert(i, (vec.clone(), via));
            }
        }
    }
    Ok(indexed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_match_beats_a_prefix_which_beats_nothing() {
        let exact = lexical_score("unpaid", "Invoice#unpaid_for");
        let prefix = lexical_score("unpa", "Invoice#unpaid_for");
        assert!(exact > prefix, "{exact} should beat {prefix}");
        assert!(prefix > 0.0);
        assert_eq!(lexical_score("payroll", "Invoice#unpaid_for"), 0.0);
    }

    /// Every query term has to pull its weight: a query that matches one of
    /// four words is a weaker match than one that matches one of one.
    ///
    /// This is also the number the fusion now multiplies by, which is what
    /// M12b changed: a name that answered a tenth of the question used to buy
    /// the same place in the ranking as one that answered all of it.
    #[test]
    fn the_score_is_a_fraction_of_the_query() {
        assert_eq!(lexical_score("unpaid", "Invoice#unpaid"), 1.0);
        assert!(lexical_score("unpaid invoices for a customer", "Invoice#unpaid") < 0.5);
        // The M12b repro's shape: one filler word out of ten, and the name
        // says nothing else about the question.
        let filler = lexical_score(
            "notice the program on disk is not the one running",
            "Class#is_app",
        );
        assert!(filler < 0.2, "{filler} is not weak evidence");
    }

    /// The hash embedder is not a trained model, so a floor measured on MiniLM
    /// vectors would be a borrowed number pretending to be evidence.
    #[test]
    fn only_a_measured_embedder_gets_a_floor() {
        assert_eq!(relevance_floor("hash"), 0.0);
        assert!(relevance_floor("onnx") > 0.0);
    }
}
