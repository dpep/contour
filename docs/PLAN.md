# Plan

## Vision

Semantic understanding of source code. Multiple representations that can be
searched, clustered, compared, summarized, and analyzed. The pipeline
progressively compresses each method:

```
source → AST → normalized AST → structural hash
                                → summary + structured metadata → embeddings
```

Every layer loses implementation detail while distilling meaning. Cheap layers
(parse, normalize, hash) run eagerly repo-wide; expensive layers (LLM
summaries, embeddings) fill lazily, on demand and budgeted, with coverage
disclosed on every answer.

The engine is the product; CLI, MCP server, Claude skill, and LSP are
interfaces on top of it (agents first).

## Target corpus

MVP: personal repos plus one large OSS Rails app (rails, discourse, or
mastodon — trekr already benches against these) as the eval corpus. Design for
monorepo scale; validate at small scale.

## Phase 1 — the pipeline, single resolution

Goal: validate the core bet (English-summary embeddings answer behavioral
queries) with an eval set, before building products on top.

1. **Scan + extract**: enumerate blobs via `git ls-files -s`, extract every
   method with lexical nesting, singleton, visibility, params, and macro
   provenance (vendored trekr extract). Facts keyed by git blob OID.
2. **Normalize + hash**: location-free structural hash of each method body,
   with local variables renamed to ordinals and literals normalized
   (rwr's compare tables, folded into a hash instead of an equality).
3. **Summarize**: one LLM call per method emitting structured output — fine
   behavior summary + typed metadata (primary purpose, secondary concerns,
   side effects, domain, patterns). Keyed by normalized-body hash + prompt
   version + model. Stored durably in SQLite. Batch API for bulk fills.
   Schema holds both a body-only and a comment-informed summary; MVP may
   generate one until the eval set shows the second earns its cost.
4. **Embed**: summaries → vectors via the `Embedder` trait (local MiniLM
   256d to start), content-keyed vector cache (gqls design).
5. **Query**: two commands to start —
   - English search: RRF fusion of fuzzy name match ⊕ summary-embedding
     similarity, with an absolute relevance floor.
   - `--similar <method>`: nearest neighbors with disclosed tier
     (structural / near-structural / semantic) and derived confidence.
6. **Eval harness**: labeled set from day one — ~50 "query → expected method"
   pairs and ~30 "duplicate / must-not-collide" pairs against the eval
   corpus. Every threshold and model choice is judged against it.

Phase 1 is Ruby-only, but the record model and extractor seam are
language-neutral from the start (DEC-012) — retrofitting neutrality after
Ruby-isms leak into the engine costs far more than designing for it now.

Phase 1 exit criteria: eval numbers for search hit-rate and duplicate
precision/recall that beat a grep/rq baseline.

## Phase 1.5 — Rust extractor + dogfooding

A thin second language, primarily to dogfood contour on contour (and the
sibling repos) while building Phase 2. Everything downstream of extraction —
summaries, embeddings, caching, search — is language-agnostic, and English
summaries put Rust and Ruby methods in the same vector space (cross-language
similarity comes free).

- tree-sitter-rust extractor for fns / impls / methods with module-path
  nesting (crib rq's Rust plugin).
- Degraded normalization tier: comment/whitespace-stripped token-stream hash
  (`how: token_hash`) instead of full normalized-AST hashing — catches
  exact-ish clones only, disclosed as such. Full Rust normalization only if
  dogfooding shows the dedup tier matters there; don't let parity become a
  tarpit.
- The Ruby testbed provides labeled ground truth; Rust dogfooding provides
  live usage friction. Complementary, not substitutes.

## Sequencing note (recorded, because the plan moved)

Phase 4's agent surface was pulled forward ahead of the summarize/calibration
work. The owner chose the MCP server and canonicality over unblocking
summaries with an API key, so:

- **Parked on a key:** the uniform rails fill, the relevance-floor calibration
  it would settle, and any judgement about summary quality. Everything needed
  for them is built and tested against fixtures.
- **Pulled forward:** the MCP server and Claude skill (Phase 4), and
  canonicality signals (Phase 3).

The reshuffle is what made DEC-018 possible: with sessions on the MCP surface,
summaries arrive by grazing rather than by purchase, so the expensive layer
fills without the key it was blocked on. The clean calibration number still
wants a uniform fill — see DEC-018 on why provenance keying keeps that
available.

## Phase 2 — similarity products

- Duplicate detection, tiered: exact structural-hash clones →
  near-structural → semantic-only. Each result discloses how it was found.
- Threshold calibration: rerun gqls's relevance-floor experiment on method
  summaries (answerable vs nonsense queries; duplicate vs distinct pairs).
- Clustering over embedding space; scope filters (class / module / dir /
  repo).
- Container centroid embeddings: every class / file / dir / repo gets a
  running-mean embedding of its children's vectors (ae's incremental-mean
  trick) — zero LLM cost, powers clustering and coarse-to-fine routing
  before rollup summaries exist.
- Embedding bake-off: MiniLM-on-summaries vs code-native models
  (voyage-code, jina-code) on bodies and on summaries, judged on the eval
  set. Keep the winner per use-case.

## Phase 3 — hierarchical zoom + canonicality

Multi-resolution via hierarchy: coarser units, not blurrier methods
(DEC-013). Zoom is repo → dir/namespace → file/class → method.

- Rollup summaries: containers summarized bottom-up from their children's
  summaries + structure, Merkle-style keyed (a container's summary keys off
  its children's summary keys, so an edit invalidates exactly the path up
  the tree). Two trees over the same leaves: namespace (semantic; reopened
  classes via trekr's sites) and file/dir (physical).
- Coarse-to-fine query routing: route the query to top-k containers, then
  search methods within — as a ranking bias, never a hard prune, with
  narrowed scope disclosed. The monorepo-scale story without ANN.
- READMEs/docs as *doc-derived* container summaries, stored beside
  code-derived rollups, never blended (DEC-008 discipline) — the comparison
  is drift detection at directory/repo granularity.
- Canonical implementation ranking: reference counts (trekr `--refs`), git
  age, namespace centrality.
- Resolved ownership (trekr `Tree`) as a ranking/grouping signal beyond
  lexical nesting.
- Coarse per-method summary levels — only if evidence shows hierarchy +
  metadata facets still can't serve some concept queries.

## Phase 4 — surfaces + analysis

- MCP server + Claude skill (the primary daily consumer).
- Documentation drift: comment embedding vs body-only summary embedding.
- Repo health metrics — each derived from a measured quantity with a defined
  denominator, never a flat score.
- LSP last.

## Milestone 11 — DELIVERED

**All four rulings below are built and measured.** The sections are kept for
the evidence behind them; the decisions themselves live in `docs/DECISIONS.md`.

| ruled | built | where |
| ----- | ----- | ----- |
| 1. Path classes | 11a | DEC-021, DEC-022 — `src/paths.rs`, extended in 11c to classify Rust test *units*, not only files |
| 2. The near tier's measure and threshold | 11b, re-ruled 11c | DEC-023 and its postscript — `saves_nodes` measures, `shapes` decides, 0.70 stands on per-population evidence |
| 3. Label methodology | 11b, 11c | `tests/eval/README.md` — the sweep, `similar.tsv` and `pairs_short.tsv` scored, rails' near labels re-audited by census |
| 4. Context-dependent constants | 11c | DEC-024 — `src/constants.rs`, built narrower than approved, with the reason measured |

Two things M11 also learned that were not on its list: the blend weights a
summary above an identifier (DEC-023), and Rust's inline tests were never
reached by the test discount. Both were live complaints from field trials
rather than plan items, which is the argument for keeping the trials running.

**What M12 inherits** is everything under "The recurring shape" and "Still open,
and genuinely so" below. The shortest version: the near tier is honest on app
code and swamped by test siblings that no threshold separates; three separate
findings turned out to be one question about names; and the eval can no longer
compare its two candidate measures without labels sourced from the other one.

### The north star, ruled by the owner

> "Getting us into the right neighborhood is probably our best bet — we can't
> do everything, but we can optimize the search."

contour's job is to make the **first step unmissable**: recall, ranking, and
honest disclosure. The last step — reading the code and judging it — belongs to
the person or agent, and the rq field trial showed that division working end to
end ("the tool got me to the neighbourhood and manual reading did the last
step").

The practical weight of this: **the search-ranking work is the product
investment**, ahead of any completeness chase. A feature earns its priority by
how much it improves the pointing. That is why the buried-summary repro below
outranks a second language, a deeper normalization tier, or another surface.

M11 is briefed **after the owner's live MCP test drive**, deliberately. Real
usage ranks the scope, and the first grazed summaries (DEC-018) will un-idle
the queries whose answers never appear in an identifier — which is the half of
`search` no eval has been able to exercise yet. Expect the brief to reorder
what follows; expect it not to reopen it.

### 1. Path classes — the centerpiece (DEC-021, DEC-022)

Four findings across three corpora are one absent concept: contour has no
notion of what *kind* of file it is looking at. The defaults are ruled:

- **migrations** — ignored by default
- **generated, vendored** — ignored by default
- **tests and fixtures** — **included**, tagged, and ranked as a separate
  population. Not ignored. Test duplication is real maintenance signal, and
  "just ignore tests" is the tempting wrong answer.

Every default disclosed on every run (`N group(s) in ignored paths withheld`)
and every default overridable — healthy defaults, config, options.
Classification attaches at the **file** layer, never the blob layer.

The **Rust free-function owner gap** is a sibling of this work, not a separate
errand: the module prefix that would disambiguate five identically-named
`tests::find` functions lives in the path, which is why the extractor has never
been able to reach it. Same layer, same seam, do them together.

**Landed in 11a** (`paths::Class`, `paths::Classes`, `.contour.toml`), except
the owner gap, which is deliberately **not** in that commit: it changes what a
unit is *called*, so it moves every surface that prints an id and invalidates
the `git<TAB>git` labels in the Rust eval sets. Behaviour or structure, not
both — it wants its own step.

Measured old → new, on the same checkouts:

| corpus | number | before | after |
| ------ | ------ | ------ | ----- |
| discourse | exact precision | 0.73 (11/15) | **1.00 (11/11)** |
| discourse | exact recall | 1.00 | 1.00 |
| rails | search top-1 / top-5 | 1/77, 2/77 | **6/77, 12/77** |
| rails | exact precision | 0.93 | 0.93 (its FP is the constant-scope pair) |
| six Rust sets | every number | — | unchanged |

One bonus finding, recorded because it is evidence for the `generated` class
beyond the ruling: rails' two `db/schema.rb` files contributed **144 phantom
units**. The schema DSL's `t.string "status"` reads as an attribute macro, so
the extractor minted `ActionMailboxInboundEmail#status`, `#status=` and
`#status?` — a class that does not exist, with methods nothing defines. Same
shape as the discourse serializer finding, and classifying the file removes the
whole population from search without touching the macro table.

Two limits worth knowing, both honest rather than fixable by a rule:

- `CommentMigration#up`/`#down` lives in `lib/comment_migration.rb`, so no path
  rule can see that it is a migration. It is still a near-tier false positive,
  and discourse's near precision is still 0.83 because of it.
- A Rust `#[cfg(test)] mod tests` sits in an app file. Classification is a pure
  function of the path, so those units are `app`. Fixing that means an
  owner-aware rule, which is the free-function owner gap above.

### 2. The near tier — a different measure, not a different number

Ratified direction: **payoff against effort, both node-denominated.** Shared
nodes are what consolidating buys; differing nodes are what it costs. Explore
edit distance normalized by size. **Jaccard is demoted toward candidate
generation**, which is the job it is genuinely good at — the inverted index
already uses it that way and that part scales.

Recalibrate against the **merged two-corpus labels**, not rails alone. The
evidence this is a measure problem rather than a threshold problem: 0.80 misses
11 of 13 genuine short near-duplicates, and both labeled false positives sit at
*exactly* 0.80, so moving the constant trades one failure for the other and
settles nothing.

The `up`/`down` case — bodies that are structurally near-identical and
semantically opposite — gets **a sentence in the docs, not engineering**. No
structural measure can see the opposition, and saying so is cheaper and more
honest than a special case that would half-work.

### 3. Label methodology (see `tests/eval/README.md`)

**A label is never sourced from the feature it evaluates.** rails' near labels
were drawn from the near tier's own report, so they could only confirm it —
which is why rails showed 1.00 recall and discourse, labeled by sweeping down
to 0.55 and reading what turned up, showed 0.50. rails' near labels need a
discourse-style re-audit before either number means anything.

### 4. Context-dependent constants — caveat, never fold

**The labeling rule** (ratified, and written up in `tests/eval/README.md`): a
pair is a duplicate iff consolidating it would **reduce net complexity**. A
consolidation that needs metaprogramming or a behaviour change is not a
finding; it is a wasted trip.

rails' `Compatibility::V7_0#compatible_table_definition` and its `V6_1` sibling
are byte-identical and group as exact clones, but each body's unqualified
`prepend TableDefinition` resolves to its own version module's nested class, and
each `super` continues a different ancestor chain. Every copy exists to *be* a
distinct link in that chain, so the offered consolidation does not exist. This
is DEC-017's cousin, and `--min-lines` cannot fence it: the false collision is
6 lines and the smallest labeled true duplicate is 5.

DEC-017 folded `super`'s enclosing name into `norm_hash` and that was right,
because `super` is **always** name-dependent. An unqualified constant is only
**sometimes** context-dependent, and the difference is measurable rather than
arguable. On rails' 296 exact groups:

| policy | groups affected |
| ------ | --------------- |
| fold the lexical nesting at every unqualified constant read | **splits 127 (43%)** |
| caveat every group whose bodies read an unqualified constant | flags 165 (56%) |
| caveat only where such a constant is defined under **more than one** nesting | flags **54 (18%)** |

The fold destroys 43% of the report to fix a population inside 18% of it —
mostly cross-class clones referencing a top-level `Hash` or `ActiveSupport`,
which is exactly the detection `norm_hash` excludes the owner in order to get.
A blanket caveat at 56% is noise nobody reads.

**Approved by the owner for M11.** The caveat-only variant, which is the third row: leave `norm_hash` alone — no reindex, no
resummarize, DEC-003's key stays a pure function of the body — and add a
disclosed caveat tier to the *report* for a group whose members sit under
different nestings and read a constant that is defined under more than one.
Knowing which constants those are needs a constant table, which DEC-014 drops
at the seam; the cheap way is to shell out to `rq`, for which canonicality has
now set the precedent (measured, degrades to unavailable, never to a guess),
and the durable way is Phase 3's namespace tree.

It composes with canonicality: a group whose consolidation may not be available
should not be crowned without saying so.

And it defines the labeller's hardest judgement out of existence. Today someone
has to decide per pair whether the offered refactor is real; with the caveat in
the output, the tool discloses the uncertainty and the reader applies the rule.

## Milestone 12a — DELIVERED

Contribution resilience: the flywheel had one door, and that door could be shut
for a whole session.

| shipped | where |
| ------- | ----- |
| a resident MCP server restarts into a contour installed underneath it | DEC-025 — `src/mcp.rs` |
| `contour pending` / `contour store-summary`, same gates, one payload | `src/cli/mod.rs`, `summary::contributed::accept` |
| `side_effects: raises` means "signals failure to its caller" in any language | `summary::SIDE_EFFECTS`, the skill, two frozen tests |
| `--status` takes a path | `store::checkouts` |

Verified against the real thing rather than only the suite: two optimized builds
swapped under an open stdio session (same pid, healed, tools re-announced), and a
live MCP round that contributed two summaries to the real index. The second is
the DEC-018 bet paying off in public — the query "notice that the program on disk
is not the one running" shares no word with `superseded`, returned three
unrelated test functions before the contribution and returned `superseded` first
at `[semantic/summary] cos 0.47` after it.

### Awaiting a ruling

**1. Version the derived database in its filename** (DEC-025's last section). The
restart *recovers* from schema skew; this would make it impossible, and would
also retire the "two contours take turns wiping each other's index" hazard
`Store::init` guards against. It is an on-disk restructure, so it is the owner's
call rather than an implementer's.

**2. ~~The Rust free-function owner gap.~~ RULED AND BUILT** — see DEC-026. The
owner took it as a uniquely-cheap-moment argument: it cost three surfaces, and
the price was re-keying 42 Rust summaries, a number that only grows. Composed at
the file layer per DEC-021, so layer 1 stays byte-pure and no reindex is needed.

## Milestone 12b — DELIVERED

The ranking milestone. Six items, all landed; two of them did not land as
briefed and the difference is the interesting part.

| shipped | where |
| ------- | ----- |
| the fusion hears both halves' measurements (**IDF built, measured, not shipped**) | DEC-027 — `src/search.rs` |
| a unit knows who may call it | DEC-028 — `core::Visibility`, both extractors, schema 11→12 |
| a container answers for its one public unit | DEC-029 — `src/search.rs` |
| one rule for what kind of code a unit is | `Classes::of_unit` at all four sites |
| MCP results are compact | `src/mcp.rs`, one line, every tool |
| the eval asks questions the way a person types them | `queries_natural.tsv`, `contour:natural` |
| read/write axis | designed and recorded below, nothing built |

Verified against the installed binary and a live MCP round, not only the suite:
the repro ranks correctly through `~/.cargo/bin/contour` against the real index,
and one stdio session returned the new `lexical` and `nominated` fields, a
compact `symbols` payload with no nulls, and a correctly-refused contribution.

### The two things a successor should know before touching ranking

**1. The measurement is the weight, twice over.** DEC-023 gave the semantic half
a weight because RRF discards the cosine; M12b found the lexical half had the
same problem and no voice at all, and then found that *containers* had it too.
In all three cases the fix was to multiply the RRF term by something already
measured rather than to add a constant. When a new evidence source joins the
fusion, this is the first question to ask about it — and the second is whether
its measurement means the same thing as the others', because a
container-lexical half was built on the assumption that it did and swept the top
of every ranking (`lexical_score` over a thirty-word container text is not
`lexical_score` over a three-word name).

**2. Do not read the fast sets as the result.** The seven Rust sets, `fixture`
and `mastodon` run in minutes; `rails` and `discourse` take hours, and they are
where a change stops flattering itself. Nomination looked like +3 top-5 on the
nine fast sets and is +2 across all eleven, because rails does not move at all.
DEC-027 looked good on the fast sets and was *better* on rails. Run all eleven
before writing a number down.

### What this milestone learned that was not on its list

- **A census that approximates the tool's own rule is not a census.** The
  nomination rule was validated by a Python scan of Ruby source that counted
  only `def`, and it said 9 of 20 labeled answers were their container's sole
  public method. contour disagreed, because Rails classes carry `attr_reader`
  and macro-generated units are public: the first build of nomination did
  nothing at all for `BackupService`, the exact case it was written for, and
  did it silently.
- **Natural phrasing is not uniformly harder — it is coverage-dependent.**
  Against summaries a full sentence beats a keyword query; against a cold corpus
  it loses, because the filler is tokens no identifier can match. That is
  DEC-018's flywheel argued from the eval instead of from first principles, and
  it is now in the skill as advice a session can act on.
- **`script/check.sh` piped into `tail` reports success when it failed.** Its own
  header warns about this and it still landed a red commit. Redirect and check
  `$?`.

## M12b material: the lexical half scores on words that carry no signal

**A fresh repro, on a corpus that is actually summarized** — which the DEC-023
repro on rq never was. `contour search "notice the program on disk is not the
one running" src --floor 0` puts `mcp::superseded` **third**, behind
`paths::Class::is_app` (cos 0.33) and `paths::is_module_name` (cos 0.24), while
`superseded` carries the highest cosine on the page at **0.46** and is the only
unit whose summary answers the question.

The mechanism is measurable and it is not DEC-023's. Strip the common tokens and
the ranking is clean:

| query | `superseded` | what else is in the top 3 |
| ----- | ------------ | ------------------------- |
| `binary replaced disk detect` | **1st**, cos 0.52 | genuine semantic neighbours |
| `is the binary replaced on disk` | 1st, cos 0.49 | `is_module_name` (0.17), `is_unqualified` (0.12) |
| `notice the program on disk is not the one running` | **3rd**, cos 0.46 | `is_app` (0.33), `is_module_name` (0.24) |

One `is` in the query is enough to pull two units into the top three on the
strength of a token that appears in a large fraction of every corpus's
identifiers. DEC-023 weighted *which vector* answered; this is the other half —
the lexical scorer has no notion of how informative a token is, and RRF gives it
equal standing regardless.

Two notes for whoever picks this up:

- **Qualified ids (DEC-026) make it slightly worse and did not cause it.** A
  longer id carries more tokens, so more units match something. The seven Rust
  eval sets moved 7/21 to 8/21 top5 across that change, so this is an anecdote
  the eval does not yet capture — which is itself the finding: no labeled query
  contains a stopword-heavy phrasing, and a person's would.
- **The obvious shape is IDF over the identifier corpus**, which is cheap
  (the identifiers are already tokenized and embedded per checkout) and
  falsifiable against all eight query sets. Do not tune `RRF_K` by feel first;
  this says the input to the lexical half is wrong, not its weight.

### RULED BY MEASUREMENT: it was the weight, and IDF did not earn itself

Built and scored all four ways against every labeled set (DEC-027). **The
diagnosis above was half right and the prescription was wrong.** The lexical
half's *input* was fine; what was broken is that the fusion never heard its
output. `lexical_score` has always returned "how much of the query this name
accounts for", and RRF threw it away — so `is_app` matching one `is` out of ten
words bought the same place as a name that answered the question outright, and
with `RRF_K` 60 it went on doing so about 135 places down the list. Multiplying
each half's RRF term by its own measurement is what fixes the repro, and it is
not a tuning of `RRF_K`: it is DEC-023's argument applied to the other half.

IDF was built twice, because the first shape had a real flaw and the second
exposed a bigger one:

| variant | fixture t1/t5 | 7 Rust sets | mastodon | discourse | rails | total t1/t5 |
| ------- | ------------- | ----------- | -------- | --------- | ----- | ----------- |
| shipped before | 7/12 | 6/7 | 2/4 | 0/1 | 6/10 | 21/34 |
| **weight only (shipped)** | **10/21** | 6/8 | **3/4** | 0/3 | **11/21** | **30/57** |
| IDF only | 7/12 | 6/9 | 2/5 | 0/1 | — | — |
| IDF + weight | 7/14 | 6/9 | 3/4 | **0/5** | — | — |

Denominators: fixture 22, Rust 21, mastodon 21, discourse 25, rails 77. `found`
is identical in every column — **every movement is rank, not recall.** rails was
scored on the shipped pair only; at 77 queries over 54k units it is an
hours-long run per variant, and the two IDF variants had already been decided
against on the ten sets above.

**rails is the strongest confirmation and the least surprising one.** It is the
largest corpus, so it has the most units whose long names accidentally share a
word with a question, and it is where the old fusion had the most places to hide
an answer behind them: top-1 6 → 11 and top-5 10 → 21, both close to doubled,
with the same 77 answers found either way.

**Why IDF backfired, which is the part worth keeping.** With the weight in
place, the denominator of `lexical_score` decides how much a partial match is
worth, and IDF has to choose what to do with a query word no identifier
contains. Both available answers are bad:

- *Count it at full weight* (the textbook clamp): on a natural sentence most
  words are unmatchable, so every score is crushed toward zero by roughly the
  same factor. Measured identical to the weight alone on every set — IDF bought
  nothing.
- *Leave it out of the denominator*: a seven-word question becomes a two-word
  one. On the fixture corpus, `retries a few times before giving up` left `a`
  (which prefix-matches `available`, `account`, `authenticate`, …) as half the
  askable query, and six unrelated units tied at `name 0.50`. That is the
  0.50-shaped rubbish in the table's last row.

The whole query is the honest denominator: a name that accounted for one word
of seven accounted for a seventh, whether or not the other six are findable
anywhere.

**IDF is not refuted, it is unmeasurable here**, and the reason is the next
item on the milestone list. It was ahead on discourse (0/5 against 0/3) and
level on the Rust sets, and behind only on the one corpus that is both small and
fully summarized. No labeled query in any set is phrased the way the repro is —
which is exactly the gap `queries_natural.tsv` exists to close. **Re-ask this
question against that band before deciding IDF is dead**; the four binaries and
the harness are in the milestone's scratch directory.

## M12b: entry-point ranking, and what the census actually said

**The brief's "11 of 21 mastodon answers are `ServiceObject#call`" was the
navigation trial agent's self-count**, not the label set; the owner ruled the
census below supersedes both numbers. The labels hold 21 queries of which four
are answered by a `#call`.

Two censuses, both against a real mastodon checkout, and the second corrected
the first:

1. **Rank of the labeled answer against the best-ranked unit of its container.**
   Answer in top-5 4/21; some unit of the right container in top-5 7/21; the
   container out-ranks its own entry point on 6/21. Four labels have no unit of
   the right container ranking at all — nomination cannot help those, and they
   are what summaries are for.
2. **Is the labeled answer its container's sole public method?** A first pass
   scanning Ruby source said 9 of 20 — and it was **wrong in the optimistic
   direction**, because it counted only `def` and Rails classes carry
   `attr_reader`. `BackupService` has three public units by contour's reckoning
   and nominated nobody until macro-generated accessors were excluded. A census
   that approximates the tool's own rule is not a census of the tool.

The phenomenon is sharper than `#call`: an entry point is named for the
**protocol** and its private helpers for the **behaviour**. DEC-029 records what
was built, including the container-lexical half that was built and dropped.

## The read/write axis — DESIGN ONLY, nothing built

Recorded at M12b as briefed: **specify what would validate it, build nothing.**

The question is whether contour can answer "which of these *changes* something"
against "which of these just *reads*" — the distinction behind "where do we
settle an invoice" (a write) versus "where do we work out what is owed" (a
read), which today are the same query to the ranker.

**The discriminator may already exist and be free.** Every contributed summary
carries `side_effects` from a closed seven-word vocabulary
(`summary::SIDE_EFFECTS`): `persists`, `network`, `filesystem`, `mutates`,
`observes`, `raises`, `spawns`. A unit whose summary names `persists` or
`mutates` writes; one that names neither, or only `observes`, reads. That is a
fact already bought and already stored, in the purchased half, on every summary
anyone has grazed. Nothing needs to be built to *have* it — only to use it.

### What would validate it, before anything is built

Three things, in this order, and the first two are measurements rather than
code:

1. **Coverage.** The facet is worth nothing at `coverage none`, which is every
   corpus in the eval except `fixture`. Count how many summaries in the
   purchased half name a side effect at all, and how the seven words are
   distributed — a vocabulary where 90% of rows say `persists` discriminates
   nothing. **This is the gate**: below some real coverage the rest of the list
   is unanswerable, and the honest answer is "come back when grazing has run
   longer".
2. **Agreement.** Sample summarized units and judge by reading whether
   `side_effects` matches what the body does. It is a *model's* claim about a
   body, contributed by whatever model was running (DEC-018 keys contributions
   by model for exactly this reason), and no one has ever checked it against the
   source. A facet that is right 70% of the time is a filter that lies to a
   reader once every three answers.
3. **Labels that ask the question.** Neither `queries.tsv` nor
   `queries_natural.tsv` contains a read/write pair — two queries over the same
   domain whose answers differ only in whether they mutate. Half a dozen such
   pairs per corpus, drafted the way every other label is, would say whether the
   axis separates anything a person actually asks for. Without them any
   mechanism can only be argued about.

### The three shapes it could take, and what each costs

Recorded so the choice is made once rather than drifted into:

- **A ranking signal** — `search` biases toward matching side effects. Cheapest,
  and the worst of the three: it would be a fourth weight in a fusion that DEC-027
  has just finished making legible, and a query that does not care about the axis
  would silently pay for one that does.
- **A facet filter** — `search --writes` / `--reads`, disclosed and off by
  default, answering from `side_effects` where it exists and *saying so where it
  does not*. Composes with DEC-009's coverage disclosure, and it is honest at
  partial coverage in a way a ranking bias is not: a filter can report "14 of 60
  candidates had no summary to filter on", where a weight just quietly ranks them
  wrong. **This is the shape to build if any is.**
- **A metadata facet in the record** — promoting read/write out of `side_effects`
  into its own column. Rejected in advance on DEC-016's arithmetic: it would be a
  key-adjacent change in the purchased half to store something derivable from what
  is already there.

**The trap to avoid**, named because it is the tempting one: deriving the axis
from the *body* instead of the summary — `save`, `update!`, `<<`, an assignment
to `@x`. That is a fifth language-specific heuristic living where DEC-012 says
only extraction and normalization may be language-specific, it would disagree
with the summary's own claim on some units, and contour would then have two
answers to one question. If `side_effects` is not good enough, the fix is a
better prompt, not a second oracle.

## The recurring shape: identical bodies that mean different things

Worth naming, because it has now arrived three times wearing three different
faces, and each time it was treated as a one-off:

| what the meaning depends on | corpus | mechanism |
| --------------------------- | ------ | --------- |
| the **enclosing name**, via `super` | rails | folded into `norm_hash` (DEC-017) |
| the **lexical nesting**, via an unqualified constant | rails | a report caveat (DEC-024) |
| the **method's own name**, via Rails convention | mastodon | nothing yet |

The third is the mastodon set's new false-positive class: byte-identical mailer
actions — a seven-strong `UserMailer` group, plus `AdminMailer`'s trio — whose
template lookup and i18n subject key both derive from the method name. The
bodies are identical and the behaviour is not.

The three mechanisms are not interchangeable, and the reason is instructive.
`super` is **always** name-dependent, so folding is right. An unqualified
constant is **sometimes** nesting-dependent, so a caveat is right and a fold
would destroy 43% of the report. A mailer action is name-dependent **only
because a framework says so** — and folding the method name into the hash would
delete the tool's entire premise, since finding the same body under two names is
what `dupes` is *for*.

So the open question is not "how do we fix mailers". It is whether contour
should have **one concept** for "this body's meaning depends on where it is
written", with the fold/caveat choice made per source of dependence, rather than
a third bespoke answer. DEC-024 already built the machinery a caveat needs; a
mailer rule would need to know what a mailer is, which is framework knowledge
contour has so far kept out. Recorded, not ruled.

## What M11c changed about what we know

Three findings from the milestone's last block of work, each measured and each
needing a decision that is not the implementer's to take.

### The near tier on rails is mostly a sibling-test detector

rails' near labels had been drawn from the tier's own report, so 1.00/1.00 was
arithmetic. An unbiased random sample of what the tier actually ships (seed 11,
20 pairs at the shipped 0.70) says **2 are real consolidations — precision
0.10**. Eighteen are sibling test methods differing by exactly the thing under
test; nineteen of the twenty are test code. Sampling below the threshold found
1 real pair in 15, so lowering it buys almost nothing.

**Read per population, that number reverses**: app-class precision at 0.70 is
0.82 (a census of all 22, not a sample). The tier is not broken; it is
overwhelmingly reporting a population that is near-identical by construction,
and which DEC-022 already ranks apart. The finding is about *test* duplication,
not about the threshold.

Three candidate responses, none taken:

1. ~~Move the threshold.~~ **Ruled: 0.70 stands.** Per-population evidence
   below; the blended number was measuring the wrong thing.
2. ~~A separate threshold for test-class groups.~~ **Measured and rejected**
   (0.14 against 0.08): sibling test methods are near-identical *by
   construction* — the shape they share is the harness — so no threshold
   separates them. DEC-022's sectioning plus payoff ranking is the mechanism
   that works, and it is already built.
3. **The edits-per-token measure** — still open, and now the only live idea for
   this population. A pair differing by one token in a 6-line body and one
   differing by one token in a 60-line body are not the same claim, and Jaccard
   scores them alike. This is what would separate a sibling test method from a
   copy-paste, if anything does.

### The 0.70 threshold, settled on per-population evidence

Ruled and closed. The alarming 0.10 was a *population artifact*: 19 of the 20
sampled pairs were test code, and DEC-022 already sections that away from app
code. Read per population — and the app population is small enough to read
entirely, 22 of the 230 groups rails ships at 0.70 — **app precision is 0.82 at
0.70 and 0.73 at 0.80**. The band nearest the threshold is the cleaner one, so
0.70 stands and raising it would lose 10 of 18 real app findings.

A second, higher threshold for test-class groups was measured and rejected:
0.14 above 0.80 against 0.08 below. Sibling test methods are near-identical by
construction, so no threshold separates them. See DEC-023's postscript.

### Which measure the near tier should use is still unsettled, and now says so

`saves_nodes` is a measurement and `shapes` still decides, because every label
in every set — including M11c's new sample — was sourced from what the shape
measure reports. **A sample drawn from one measure's output cannot compare it
with another.** What would settle it is written down: sweep the *node* measure
on discourse, read what turns up, label that too.

## Still open, and genuinely so

Recorded with measurements, awaiting a decision.

### What a second corpus did to the thresholds

discourse was labeled independently and reproduces exactly (exact 0.73
precision 11/15, near 0.50 recall 5/10, canonicality 4/5). Every number below
is from that run. None was an M10 patch — each needed a design decision.

**Findings 1, 2 and 3 have since been ruled.** They are kept here for the
evidence; the direction they became is under "Milestone 11 — settled
direction" above. Findings 4, 5 and 6 are still open.

**1. Exact precision falls 0.93 → 0.73, and every new false positive is a
schema migration.** Byte-identical `up`/`down` bodies and re-runnable
`migrate` methods across `db/migrate`. Under DEC-020 they are not duplicates
at all: migration history is frozen, so consolidating one is not a
consolidation, it is a rewrite of the past. `--min-lines` cannot fence them —
the largest false collision is 16 lines, well above any usable floor.

The fix needs contour to know that `db/migrate` is a *class* of path with
different rules. That is the same shape as two other things already recorded
here — the test-code ranking question, and the constant-scope caveat tier —
and a third instance is the labeler's dogfood finding that `.rb` fixture
corpora inside trekr and rwr index as real Ruby units and pollute their dupes
reports. Four symptoms, one absent concept: **contour has no notion of what
kind of file it is looking at.** Worth solving once, deliberately, rather than
four times in patches.

**2. Near recall falls 1.00 → 0.50, and the 0.80 threshold is now
argued-with by our own eval.** Genuine one-edit copies on discourse score
0.56–0.73, below the bar. The labeler disclosed the reason the rails number
looked better: rails' near labels were sourced *from the tier's own report*,
so they could only confirm it. discourse's were found by sweeping to 0.55 and
reading the results — which is how you find what a threshold misses.

0.80 therefore has exactly the status the relevance floor has: a number
measured on one corpus and disclosed as such. Recalibration wants both
corpora and non-circular labels on each.

**3. The short-body band needs a different measure, not a different
number.** 0.80 misses 11 of 13 genuine 4–8-line near-duplicates, and both
labeled false positives sit at *exactly* 0.80 — so moving the constant trades
one failure for the other and settles nothing. `near::NEAR_THRESHOLD` already
records that Jaccard is harsher on a small body; this is that limitation with
a denominator. The labeler's diagnosis is that the variable is
edits-per-token rather than length, which is a different metric rather than a
different threshold, and is the shape of the next attempt.

**4. Canonicality transfers, and improves.** 4/5 on discourse against 2/5 on
rails, with `git_age` at 5 correct and 0 wrong. Evidence in DEC-019's favour:
the signals are not tuned to one repo's history, and abstaining on
disagreement kept the wrong-answer count at zero on both corpora.

**5. The Rust token tier's contract holds exactly.** Six sibling repos, every
labeled collision found, zero false, and boundary pairs one token apart
behaving as designed (DEC-012). Two dogfood findings worth keeping: rq carries
the same epoch-seconds helper four times under three names, and gqls's case
helper is pasted into ae — a cross-repo duplicate contour cannot label today,
because a labeled set is scoped to one checkout.

**6. `similar.tsv` is written and waiting.** 17 assertions, 5 of them marked
as what *should* happen and currently failing — including identifier noise
outranking genuine siblings in berater's `acquire_lock` family, which is a
finding about ranking rather than about the harness. Wiring it is the
cheapest next step in the eval, because the labels already exist.

### The blend buries a summary hit under identifier noise

**Top of the ranking work, from a field trial on rq** (an independent session
ran a real dedup pass through the MCP surface and landed a green branch).
Reproducible: `contour search "throwaway repository built for one test to
index" src/tests --floor 0 -l 8`. The unit whose contributed summary literally
says "build a throwaway indexed repository for one test" ranks **fifth at
cosine 0.44**, below four identifier-tier hits at 0.38 / 0.26 / 0.25 / 0.20.
The final score is not monotonic in cosine: RRF's lexical half dominates on a
corpus of long, sentence-shaped Rust test names.

This is the flywheel's own bet failing in public — the trial's verdict was
"contributed summaries did not visibly improve search, so grazing stayed an act
of faith" — and DEC-018 depends on grazing visibly paying off.

**Path classes do not fix it, measured after 11a landed: the same query returns
the same order.** Every candidate in that scope is test code, so a uniform
discount cannot reorder them. What is left to look at is the blend itself — the
1.0 lexical / 0.7 semantic weights and `RRF_K`, both inherited from gqls — and
it wants the eval query sets plus this repro, not a retune by feel.

Two smaller findings from the same trial:

- **`saves_nodes` did not know test from production code**: a 5-line test
  helper (112 nodes across 5 copies) outranked a 4-way production clock, the
  most valuable find of the run. Fixed by 11a's populations.
- **The token-hash tier split a human-visible 4-way Rust duplication into two
  2-member groups**, because the string literals differ. Correct by design
  (DEC-012) and worth a note in the report or the near tier below.

### Rust near tier: DEC-012's "only if dogfooding proves the need"

**The proof arrived.** The same field trial's biggest wanted-and-couldn't: the
near tier would have caught a real `indexed` / `indexed_fixture` pair in rq,
which the session found by hand instead. Recorded for **M12**, not M11 — it is
a normalization tier, not a milestone about hardening.

### A serializer DSL mints writer methods that do not exist

`attribute :admin_ids` in a discourse serializer produces
`AboutSerializer#admin_ids=`, which surfaces in `similar`. The macro table
treats `attribute` as an accessor, which is right for ActiveRecord and wrong
for ActiveModel::Serializers, where it declares a serialized field and defines
no writer.

Telling them apart means knowing what the enclosing class inherits from —
resolved ancestry, which DEC-014 drops at the extractor seam on purpose and
which arrives with the tree layer in Phase 3. So this is not a macro-table
fix; recorded with the cost rather than patched with a guess about which
`attribute` is which.

### `module_function` gives one method two ids

`module_function :secure_compare` is faithfully extracted as two units — a
private instance method and a public singleton one — because that is what Ruby
does. `--symbols` on `active_support/security_utils.rb` shows both. `dupes`
already handles the pair (it groups by *span*, so the two never report as
clones of each other), but search ranks them as two answers to one question, and
`Owner#x` and `Owner.x` both name a method a reader thinks of as one thing.

Not fixed, and not obviously a bug: the singleton really is callable and really
is public where the instance method is not. The question is whether a *unit* is
a def or a callable entry point, which is DEC-014 territory.

**Cost, assessed in M10 and found not to be modest.** Suppressing the second
unit is one line in the macro table, and it is the wrong line: the pair is
real, `dupes` already handles it correctly (it groups by *span*, so the two
never report as clones of each other), and the only surface that misbehaves is
search ranking them as two answers. So the cheap fix removes a true fact to
tidy one display, and the honest fix — a unit knows it is one of several entry
points to one body — is a record-model change touching every surface that
prints an id. That is DEC-014's question and belongs with the tree layer, not
with a milestone about hardening. Parked, with the reasoning rather than a
guess.

## The guard-clause case: two mechanisms, and only one is the scorer

The field report's first finding: *"a query about 'prevent an action once a
resource is already finalized' missed the one guard-clause method that
implemented exactly that check, while several methods that merely called that
guard ranked above it."* M12b's rule says do not tune ranking on one anecdote,
so this was reproduced instead. It reproduces — twice, by two different
mechanisms, and they want opposite things.

**The case is now labeled**, in `tests/eval/fixture`: `corpus/shipping.rb` is a
one-line guard and four methods that call it, with a keyword row in
`queries.tsv` and a sentence row in `queries_natural.tsv`, both expecting the
guard. Only one of the two phrasings triggers each mechanism, which is itself
the finding.

### A. One stopword prefix-match tips the fusion — contour's fault

On a corpus holding only the five methods, `prevent a change once something has
been finalized` ranks `add_parcel` (cos 0.27) above `ensure_open!` (cos 0.42),
which carries the highest cosine on the page and is the only unit that answers
the question. Scores 0.0164 against 0.0164 — a tie broken the wrong way.

The whole difference is the word **"a"**, which prefix-matches **"add"**.
`lexical_score` gives a prefix half credit, that is 0.06 of the query, and the
0.06 buys `add_parcel` a *second* RRF term where `ensure_open!` has one. Delete
one word from the query and the ranking is right:

| query | first | why |
| ----- | ----- | --- |
| `prevent a change once …` | `add_parcel` cos 0.27 | `both`, lexical 0.06 |
| `prevent changes once …` | `ensure_open!` cos 0.38 | `semantic` only |
| `stop an update after the record is finalized` | `ensure_open!` cos 0.36 | no lexical hit anywhere |

This is **exactly the case DEC-027 parked**: "IDF is not refuted, it is
unmeasurable here… no labeled query in any set is phrased the way the repro
is." The field report supplies the phrasing the eval lacked. Two candidate
moves, neither taken, both falsifiable against all eleven sets:

- **No prefix credit for a query token of one or two characters.** `pay`
  matching `payroll` is the evidence the half credit was written for; `a`
  matching `add` is not. One line, and it attacks the trigger rather than the
  symptom.
- **IDF**, re-asked against the natural band, which is what DEC-027 said to do
  before declaring it dead.

Deliberately not chosen here: the anecdote is one query on a five-unit corpus,
and M12b's own history is a diagnosis that was half right with a prescription
that was wrong.

### B. The caller's summary out-scores the guard's — the summarizer's fault

Ask the same question in keywords over the 26-unit fixture and the guard ranks
**fourth with no lexical hit anywhere**: pure cosine puts `relabel` (0.33),
`remove_parcel` (0.33) and `reroute` (0.29) above `ensure_open!` (0.28). That
is the field report's own hypothesis — narrative similarity beating the precise
match — and it looked like the ranking's fault.

It is not. Rewrite **only the guard's summary**, changing nothing else:

| the guard's summary | its rank | cosine |
| ------------------- | -------: | -----: |
| "Signals an error unless the shipment is still open, so that nothing may change once it has been finalized." | 4th | 0.28 |
| "Refuses any change to a shipment that has already been finalized." | **1st** | **0.39** |

Both are accurate. The first describes **how the check is spelled**, the second
**what the caller is prevented from doing** — and a guard clause is precisely
the shape where those diverge, because its implementation is a negation of a
negation while its contract is a plain refusal. The callers' summaries state the
contract in passing ("refusing once the shipment has been finalized"), so they
win against a guard that states its mechanism.

That makes the cheapest lever a **line in the summarizer prompt**, not a
scoring change: for a method whose purpose is to refuse, say what it refuses.
It is also the safest, because it moves nothing that is already ranked.

### What the case cost the rest of the set

Same build, same machine, `tests/eval/fixture` before and after the five units
and two rows:

| row | before, 22 queries | after, 23 |
| --- | ------------------ | --------- |
| `contour` | 10 top-1, 21 top-5 | 10 top-1, 22 top-5 |
| `contour:identifier` | 9 top-1, 14 top-5 | 8 top-1, 14 top-5 |
| `contour:natural` | 14 top-1, 22 top-5 | 17 top-1, 23 top-5 |

The new keyword row is a **miss at top-1 and a hit at top-5** (rank 4), which is
finding B pinned; the new natural row is a top-1 hit. Everything else in those
columns is five distractor units moving unrelated queries around: −1 on the
identifier band and +3 on the natural band, from a case that added one query.
**Read that as the fixture set being too small to carry a signal**, which its
own README already says — the case is here to pin a shape, and any ranking
change it motivates has to be scored on rails and discourse.

**The caveat, and it is load-bearing.** The summaries in this fixture were
written by hand for this case, and finding B is *sensitive to the words
chosen* — which is what the rewrite above demonstrates and is the reason it is
reported as a summarizer finding rather than a ranking one. Confirming it needs
a real summarized corpus and a real summarizer, which is an API bill this did
not have. Finding A needs no such caveat: `"a"` prefix-matching `"add"` is in
the code.

## Monorepo scale, measured

The plan's non-goal said "brute-force cosine with rayon handles ~50k records
fine (measured in gqls); revisit at monorepo scale." A field trial on a
monorepo of ~2M indexed units — 40× that — is the revisit, and this is what it
cost.

**Method.** No 2M-unit corpus was available, so one was built: the Ruby of
three public checkouts (rails, discourse, mastodon — 17.7k files, 62 MB)
replicated under distinct top-level directories in one throwaway git repo, each
copy's `class`/`module` names prefixed so the copies are distinct blobs,
distinct unit ids and distinct embedding texts. Release build with the ONNX
embedder, 8 cores, 24 GB, quiet machine, `/usr/bin/time -l`. Two sizes; the
third was refused by the disk, not by the method.

| | 132,532 units | 256,402 units | per unit |
| --- | ---: | ---: | ---: |
| `index` (cold, every blob new) | 3.6 s | 5.7 s | 22 µs |
| **unscoped `similar`, cold** | **441 s** | **881 s** | **3.4 ms** |
| — of which embedding | ~430 s | ~870 s | |
| its CPU time | 3,098 s | 6,398 s | |
| its peak RSS | 757 MB | 1,219 MB | ~4.8 KB |
| unscoped `similar`, warm | 1.2 s | 2.2 s | 8.5 µs |
| unscoped `dupes --near` | 10.7 s | 24.9 s | (see caveat) |
| derived database | 623 MB | 1,141 MB | ~4.5 KB |

**Where the time goes: embedding, and it is not close.** Cold minus warm is 97%
of the cold run, and the embedding pool ran at 7.0–7.3× on 8 cores — the ~10
cores at 100% the field report saw, reproduced. The rate is **~295 units per
second** at both sizes, and everything above is linear in units to within 4%
over a 1.9× range.

Linearly extrapolated to 2M units, which is the only honest way to state it and
is stated as an extrapolation: an unscoped cold `similar` or `search` is
**about 110 minutes and about 9 GB of resident memory**, with a derived
database near 9 GB. That is the 20+ minutes with no progress signal the field
report described, and it is not a tuning problem — it is a corpus-sized
inference run standing between a caller and their first answer.

**Three things this ruled out.** The suspicion list was embedding, loading
vectors out of SQLite, and brute-force cosine. Only the first is real at this
scale: the warm run — which loads every vector and scores every cosine — is
1.2 s at 132k and 2.2 s at 256k, so the cosine scan the non-goal was written
about is still not the problem at 40× its stated limit. Loading vectors *is*
the warm cost, and it was a floor `scope` did not lower: `Store::vectors` read
the whole vector table regardless of scope. At 132k that floor is about a
second; at 2M it is the next thing to measure. **Measured and lowered — see
directly below.**

### The warm floor, measured before and after scoping the vector read

The sentence above — "`Store::vectors` reads the whole vector table regardless
of scope… at 2M it is the next thing to measure" — is what DEC-033 answered.

**Method.** The same synthetic corpus, rebuilt to the same recipe at one and
two copies (132,534 and 265,068 units), fully embedded with `contour embed` and
the ONNX embedder so every query below is warm. Release build, 8 cores, quiet
machine, `/usr/bin/time -l`. Each figure is the **median of five runs with a
sixth, cold-cache run discarded**; the two binaries are the commit before the
change and the commit after it, run against the same database. The small scope
is one directory — 47 files, 391 summarizable units.

| warm `search` | 132,534 units | | 265,068 units | |
| --- | ---: | ---: | ---: | ---: |
| | wall | peak RSS | wall | peak RSS |
| **one directory**, before | 0.58 s | 315 MB | 1.00 s | 476 MB |
| **one directory**, after | **0.36 s** | **193 MB** | **0.59 s** | **266 MB** |
| whole checkout, before | 1.00 s | 487 MB | 1.88 s | 806 MB |
| whole checkout, after | 1.05 s | 488 MB | 1.92 s | 814 MB |

**A scoped answer costs about 40% less and holds about 40% less**, at both
sizes, and the whole-checkout case is unchanged — the 2–5% on those two rows is
inside the run-to-run spread, which was ±0.2 s. That last row is the constraint
the design was built against: reading the table through is the right strategy
when the request *is* most of the table, and DEC-033 keeps it by abandoning the
read-through only once it has visited more rows than looking each key up would
have cost.

**Two things this does not fix, and both are visible in the table.** A scoped
query still grows with the corpus — 0.36 s to 0.59 s for the same 391 units —
because `Store::units` reads every unit in the checkout and filters by path in
Rust. That is now the floor, and moving it means writing `paths::under`'s rule a
second time in SQL, which is the trade DEC-033 declines to make blind. And the
whole-checkout row is what it always was: an unscoped query on a monorepo is
expensive because it is unscoped, which is the case DEC-030 and DEC-032 exist to
steer away from.

**What the fill itself cost, on the same two corpora**, since `contour embed`
(DEC-034) is what made them warm:

| `contour embed` | 132,534 units | 265,068 units |
| --- | ---: | ---: |
| distinct texts | 117,556 | 223,005 |
| wall | 450 s | 943 s |
| rate | 261/s | 237/s |
| peak RSS | 740 MB | 851 MB |

The rate is 10–20% under the 295/s the table above recorded for the same work
done inside a query, and the machine was not as quiet — three release builds ran
during the second fill. The peak memory is the interesting number: **851 MB at
265k units against 1,219 MB for the query that embeds the same corpus**,
because the fill holds one batch of 500 texts at a time where a query holds the
whole corpus's texts and vectors at once. Batching for resumability bought a
lower ceiling as well.

**The `dupes --near` figures carry a caveat that matters.** Normalization sees
through the perturbation — renaming a class does not change a normalized body —
so the replicated corpus holds ~50k *distinct bodies* at both sizes, and the
near tier's candidate-pair count barely moved (160,834 → 161,000 out of 1.29
billion possible). What those two numbers measure is the I/O half growing with
units; the combinatorial half was never stretched. **A real monorepo has ~2M
distinct bodies, and nothing here says what `near::pairs` costs on that.** It
is the one part of the field report this measurement does not answer.

## Non-goals (for now)

- Languages beyond Ruby and Rust (the extractor seam is pluggable; rq shows
  the multi-language registry shape).
- ANN indexes — brute-force cosine with rayon handles ~50k records fine
  (measured in gqls). Revisited at 256k above, and still not the bottleneck:
  the cold cost is embedding, not scoring.
- Runtime traces, coverage, ownership signals — future ranking inputs.
