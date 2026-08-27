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

## Milestone 11 — settled direction

**Ruled by the owner. These are decisions the M11 engineer inherits, not
debates to reopen.** Each has a measurement behind it, recorded below and in
`docs/DECISIONS.md`; none has been implemented.

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
