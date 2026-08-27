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

## Open questions awaiting a decision

Recorded rather than settled. Each has a measurement behind it and none has
been implemented.

### Context-dependent constants: disclose, do not fold

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

**The proposal is the third row**: leave `norm_hash` alone — no reindex, no
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

### `module_function` gives one method two ids

`module_function :secure_compare` is faithfully extracted as two units — a
private instance method and a public singleton one — because that is what Ruby
does. `--symbols` on `active_support/security_utils.rb` shows both. `dupes`
already handles the pair (it groups by *span*, so the two never report as
clones of each other), but search ranks them as two answers to one question, and
`Owner#x` and `Owner.x` both name a method a reader thinks of as one thing.

Not fixed, and not obviously a bug: the singleton really is callable and really
is public where the instance method is not. The question is whether a *unit* is
a def or a callable entry point, which is DEC-014 territory. Recorded so the
next person to see a doubled search result knows it is deliberate.

## Non-goals (for now)

- Languages beyond Ruby and Rust (the extractor seam is pluggable; rq shows
  the multi-language registry shape).
- ANN indexes — brute-force cosine with rayon handles ~50k records fine
  (measured in gqls); revisit at monorepo scale.
- Runtime traces, coverage, ownership signals — future ranking inputs.
