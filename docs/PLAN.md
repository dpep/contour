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

## Non-goals (for now)

- Languages beyond Ruby and Rust (the extractor seam is pluggable; rq shows
  the multi-language registry shape).
- ANN indexes — brute-force cosine with rayon handles ~50k records fine
  (measured in gqls); revisit at monorepo scale.
- Runtime traces, coverage, ownership signals — future ranking inputs.
