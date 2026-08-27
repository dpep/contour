//! English queries, and nearest neighbours.
//!
//! Two halves, fused. The **lexical** half matches the query against a unit's
//! name; the **semantic** half matches it against the embedding of that unit's
//! summary. Reciprocal Rank Fusion merges them (gqls's `combine`, K=60): it is
//! scale-free, so a token-overlap count and a cosine need no common
//! normalization, and a unit strong in both rises above one strong in either.
//!
//! Every answer discloses what produced it (DEC-010) and how much of the
//! corpus could have participated (DEC-009). The semantic half only covers
//! summarized units, so a search over a half-summarized repo answers from what
//! exists and says so — rather than looking like the corpus is small.

use crate::embed::{Embedder, config_key, humanize, identifier_text, mrl, summary_text, text_key};
use crate::store::{Located, Store};
use crate::summary::Coverage;
use anyhow::{Result, bail};
use std::collections::HashMap;

/// Reciprocal Rank Fusion constant, from gqls. Large enough that the top few
/// ranks are not winner-take-all.
const RRF_K: f64 = 60.0;

/// The semantic half's weight in the fusion. Below 1.0 so an exact name match
/// keeps the lead: someone typing `unpaid_for` wants that method, not the one
/// whose summary is most poetic about invoices.
const SEMANTIC_WEIGHT: f64 = 0.7;

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
    pub cosine: Option<f32>,
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
    pub floor: f32,
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

/// Which vector to prefer where a unit has both.
///
/// `Best` is what every caller but the eval wants. `IdentifierOnly` exists so
/// the eval can score the tiers against each other on one corpus — the
/// embed-code-against-embed-summary comparison DEC-004 promised — without
/// needing two indexes to do it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prefer {
    Best,
    IdentifierOnly,
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
    let class_of: Vec<crate::paths::Class> = units.iter().map(|u| classes.of(&u.path)).collect();
    let summaries = vectors_for(store, &units, embedder, prefer)?;

    // Lexical: token overlap against the humanized name, best first.
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

    let cosines: HashMap<usize, f32> = semantic.iter().copied().collect();
    let mut fused: HashMap<usize, (f64, bool, bool)> = HashMap::new();
    for (rank, (i, _)) in lexical.iter().enumerate() {
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + rank as f64 + 1.0);
        entry.1 = true;
    }
    for (rank, (i, _)) in semantic.iter().enumerate() {
        let entry = fused.entry(*i).or_insert((0.0, false, false));
        entry.0 += SEMANTIC_WEIGHT / (RRF_K + rank as f64 + 1.0);
        entry.2 = true;
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
/// and a floor of 0.4000000059604645 is an f32 artefact rather than a
/// threshold anybody chose. Rounded where the value is built (the house rule),
/// so JSON and human output cannot disagree about how much precision exists.
fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
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
    pub cosine: Option<f32>,
    /// The near tier's Jaccard, named the same as `dupes::Group::similarity`
    /// for the same reason. Two different measurements sharing one field was
    /// the other half of the drift.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
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
/// included — you can ask about a migration you are looking at. Its
/// *neighbours* follow the path policy, and say what they withheld.
pub fn similar(
    store: &mut Store,
    root: &str,
    id: &str,
    embedder: &dyn Embedder,
    limit: usize,
    classes: &crate::paths::Classes,
) -> Result<Neighbors> {
    let units = in_scope(store, root, None)?;
    let target = resolve(&units, id)?;
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
                class: classes.of(&other.path),
            });
        }
    }

    // Near-structural: mostly the same shape, between exact identity and
    // meaning. Runs before the semantic tier so a reader sees the cheaper,
    // sharper evidence first.
    {
        for near in crate::near::neighbors(store, root, here, crate::near::NEAR_THRESHOLD, None)? {
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
                class: classes.of(&near.path),
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
                class: classes.of(&units[i].path),
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
    pub floor: f32,
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

/// Token overlap between a query and a unit's humanized name.
///
/// Deliberately simple. RRF consumes a *ranking*, not a calibrated score, so
/// the elaborate fuzzy scorer gqls uses for GraphQL paths would be a borrowed
/// abstraction doing a job this does in twenty lines.
fn lexical_score(query: &str, id: &str) -> f64 {
    let name: Vec<String> = words(&humanize(id));
    if name.is_empty() {
        return 0.0;
    }
    let query = words(query);
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

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
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
    let have = store.vectors(config)?;
    let stored = store.all_summaries()?;

    let mut indexed = Indexed {
        vectors: HashMap::new(),
        text: HashMap::new(),
        coverage: Coverage::default(),
    };
    let mut missing: Vec<(u64, String)> = Vec::new();
    let mut pending: Vec<(usize, u64, &'static str)> = Vec::new();

    for (i, located) in units.iter().enumerate() {
        // A summary is only possible where there is a body to summarize;
        // the identifier tier covers everything, including the
        // macro-generated units that have no body at all.
        let summary = located.unit.norm_hash.and_then(|norm_hash| {
            indexed.coverage.summarizable += 1;
            let ctx = crate::summary::Context::of(&located.unit).hash();
            stored.get(&(norm_hash, ctx))
        });
        if let Some(summary) = summary {
            indexed.coverage.summarized += 1;
            indexed.text.insert(i, summary.summary.clone());
        }

        // Prefer meaning over naming where both exist — DEC-005's interop
        // rule, finally exercised against two real indexes.
        let (text, via) = match (summary, prefer) {
            (Some(summary), Prefer::Best) => (summary_text(summary), "summary"),
            _ => (identifier_text(&located.unit), "identifier"),
        };
        if text.trim().is_empty() {
            continue;
        }
        let key = text_key(&text);
        match have.get(&key) {
            Some(vec) => {
                indexed.vectors.insert(i, (vec.clone(), via));
            }
            None => {
                missing.push((key, text));
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
        if missing.len() > 2_000 {
            eprintln!(
                "contour: embedding {} texts with the {} embedder — this is a one-time \
                 cost per corpus and is cached; subsequent queries are instant",
                missing.len(),
                embedder.kind()
            );
        }
        // One vector per distinct text: identical summaries embed once.
        let mut unique: Vec<(u64, String)> = missing;
        unique.sort_unstable_by_key(|(key, _)| *key);
        unique.dedup_by_key(|(key, _)| *key);
        let texts: Vec<String> = unique.iter().map(|(_, text)| text.clone()).collect();

        // `None`: no `--embed-model` flag exists yet, so the spec is genuinely
        // constant and the pool resolves the same embedder `embedder` did —
        // which it must, because `config` above was keyed from that one.
        let vectors = crate::embed::embed_all(None, &texts);
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
    #[test]
    fn the_score_is_a_fraction_of_the_query() {
        assert_eq!(lexical_score("unpaid", "Invoice#unpaid"), 1.0);
        assert!(lexical_score("unpaid invoices for a customer", "Invoice#unpaid") < 0.5);
    }

    /// The hash embedder is not a trained model, so a floor measured on MiniLM
    /// vectors would be a borrowed number pretending to be evidence.
    #[test]
    fn only_a_measured_embedder_gets_a_floor() {
        assert_eq!(relevance_floor("hash"), 0.0);
        assert!(relevance_floor("onnx") > 0.0);
    }
}
