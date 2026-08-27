# Decisions

Numbered records, trekr-style. Cite as DEC-0NN in code comments when a
decision explains otherwise-surprising code.

## DEC-001 — Ruby first, parsed with prism

`ruby-prism` (Ruby's official parser), not tree-sitter. Both Ruby-serious
repos in the tap (trekr, rwr) chose it for exact semantics; rq's tree-sitter
Ruby plugin exists only because rq is multi-language. Multi-language support
is a future concern with a known shape (rq's `LanguagePlugin` registry), not
an MVP constraint.

## DEC-002 — Vendor prior art, don't depend on it

rq, trekr, and rwr all export only `pub mod cli`; none is consumable as a
library. Vendor the pieces contour needs (see docs/PRIOR-ART.md) rather than
forking to widen visibility — the vendored layers (trekr's extract, rwr's
compare tables) are self-contained and change rarely. contour itself is
lib-first: engine modules genuinely `pub` (pattern_engine is the house
example), CLI as one consumer. Agents first — MCP/skill/LSP are interfaces
over the same engine.

## DEC-003 — Two cache keys: blob OID for facts, normalized hash for meaning

Parse facts are a pure function of a blob's bytes, keyed by git blob SHA-1
(trekr's design): branch switches, rebases, and N worktrees cost nothing.
Summaries and embeddings are expensive pure functions of *meaning*, so they
key off the normalized structural hash: a rename, move, reformat, or
comment-only edit never re-summarizes.

The full summary key is `norm_hash + ctx_hash + prompt_version + model`, and
the governing law is the one gqls's vector cache taught us: **the key derives
from the exact input, so cache and content can never drift.** A summary of a
nameless body is a weaker summary, so the prompt renders structural context
(name, owner nesting, singleton, `via`) — and anything the prompt renders must
therefore be in the key. `ctx_hash` covers exactly those fields and nothing
more; widening the prompt without widening the hash is the one change that
silently serves stale answers.

The consequence to embrace honestly: **exact clones in identical context share
a summary; clones in different contexts re-summarize.** That is correct rather
than wasteful — the same body under `Payroll::` and under `Billing::` may
legitimately earn a different `domain`. And the clone *report* still comes free
from `norm_hash` alone, because grouping identical bodies never needed the
context in the first place.

## DEC-004 — Embed English summaries; validate against code-native models

Natural language collapses implementation detail and makes English queries
land in the same vector space. But this is a bet, not an axiom: Phase 2 runs
a bake-off (MiniLM-on-summaries vs code-native embeddings on bodies and on
summaries) against the labeled eval set, and each use-case (search vs
duplicate detection) keeps its winner.

## DEC-005 — Embedder seam; multi-model indexes are adjacent

`Embedder` trait (proven in ae/gqls): local ONNX MiniLM at 256 MRL dims is
the free, offline default; API providers (Anthropic, OpenAI, Voyage) plug in
behind the same trait with a batch-oriented interface. `kind() + model` is
part of every vector-cache key, so indexes from different models coexist
side by side rather than invalidating each other. The query layer decides
interop: answer from whichever index is warm, or use the cheap index for
broad recall and a bigger model to re-rank the top-k — always disclosing
which index answered. 256 dims, not 64: gqls measured that absolute
relevance floors are uncalibratable at 64.

## DEC-006 — Summarizer model is configurable, no default yet

The summarizer is the dominant cost and quality lever. Model choice (Haiku
vs Sonnet vs others) is a flag/config with the default decided by the eval
bake-off. Prompt version + model id are part of the summary cache key, so
switching models builds an adjacent summary set rather than corrupting the
existing one. Bulk fills use the batch API.

## DEC-007 — Single resolution + structured metadata first

MVP generates one fine summary plus typed metadata per method in a single
LLM call: primary purpose (one), secondary concerns (ranked), side effects,
domain, recognized patterns. This is *not* keyword extraction — fields are
typed and weighted, so a pagination concern inside a payroll method stays
secondary instead of flattening into a bag of equal keywords. Metadata
facets are the zoom-out mechanism (filter/group by domain, pattern, purpose).
Coarse summary levels (semantic mipmaps) are deferred until the eval set
shows concept-level queries failing — coarse embeddings risk collapsing
thousands of methods into indistinguishable points. Schema and cache keys
accommodate multiple levels from day one.

## DEC-008 — Comments: schema for both, drift detection uses body-only

Storage holds both a body-only summary and a comment-informed summary.
Body-only keys off the normalized hash (comment edits don't invalidate) and
keeps documentation-drift detection honest — comparing a comment against a
summary that read the comment is circular. Whether the comment-informed
summary earns its (2x) generation cost is a Phase-1/2 eval question.

## DEC-009 — Global per-machine index; expensive layers fill on demand

Index lives at `~/.local/share/contour/` (XDG), adjacent to trekr's store
and keyed the same blob-OID way — worktrees and checkouts of one repo share
everything, and the door stays open to interop with trekr's index later.
Cheap layers (facts, structural hashes) index eagerly repo-wide. Expensive
layers (summaries, embeddings) fill on demand by scope, plus budgeted
background warming (rq's `index_budgeted` pattern). Coverage
(`complete | warming | none`) travels with every answer — a search over a
half-summarized repo answers from what exists and says so.

## DEC-010 — Disclosure on every answer

trekr's contract: `status` / `confidence` / `how`, where confidence is
always derived from a measurement (cosine, tier, agreement count), never a
flat vibe, and ambiguity gets its own status rather than a low number.

**Confidence appears where the underlying judgment is graded.** A predicate is
not graded: exact structural identity either holds or does not, and attaching
`confidence: 1.0` to it would be a constant wearing a measurement's clothes —
the very thing the rule above exists to forbid. Such an answer discloses `how`
plus the evidence a reader would weigh instead. `contour dupes` is the worked
example: `how: structural`, and the group's line count, so a reader can tell a
real duplicate from two accessors that could only have been written one way.
For similarity results: `how ∈ {structural, near_structural, semantic}`.
`--explain` renders the same JSON fields, never recomputes.

## DEC-011 — Eval set before products

A labeled testbed (file-driven, trekr-style: a directory of fixtures plus an
`expected` file) exists from Phase 1: query → expected method pairs, and
duplicate / must-not-collide pairs, run against a real corpus. Thresholds,
model choices, and the multi-resolution question are all settled by eval
numbers, not intuition.

## DEC-012 — Language-neutral core; Rust as the dogfood language

Only extraction and normalization are language-specific — everything
expensive (summaries, embeddings, caching, search) operates on text and
vectors. The record model and extractor seam are language-neutral from
Phase 1 even while Ruby is the only implementation. Rust lands in Phase 1.5
as a thin tree-sitter extractor so contour can dogfood on itself and its
sibling repos; its normalization is a degraded token-stream hash
(`how: token_hash`, exact-ish clones only) rather than Prism-grade
structural hashing — full parity only if dogfooding proves the need.
English summaries make cross-language similarity free: Ruby and Rust
methods share one vector space.

## DEC-013 — Multi-resolution via hierarchy, not blurrier methods

Coarse per-method summaries collapse thousands of methods into
indistinguishable points ("reads data"). Hierarchical summaries change the
*unit* instead: class/module → file → dir → repo, each container summarized
bottom-up from its children's summaries + structure. Fewer containers keep
coarse embeddings discriminative, and "explain this namespace" falls out
for free. Merkle-style keying (container summary keyed by children's
summary keys) makes invalidation propagate up the tree incrementally.
Below the LLM tier, running-mean centroid embeddings per container (ae's
trick) cost nothing and power routing/clustering. Two hierarchies over the
same leaves — namespace (semantic) and file/dir (physical) — because Rails
makes them disagree in ways that matter (`app/models/` the directory is
uninformative; `Payroll::` the namespace is not). Coarse-to-fine routing
is a ranking bias, never a hard prune. Docs/READMEs become doc-derived
container summaries stored beside code-derived rollups, never blended —
their disagreement is drift detection at container granularity.

## DEC-014 — A unit is a callable

The record everything downstream operates on is `core::Unit`: one callable
span of source. Classes, modules, constants, ancestry edges, and call sites
are extracted by the Ruby layer and dropped at the seam. Phase 1 asks no
question about them, and a record that carries a fact invites a query that
depends on it — which is how a language-neutral core acquires Ruby-isms.

Containers arrive in Phase 3 with rollup summaries as the reason (DEC-013);
call sites arrive when canonicality ranking needs reference counts. Both are
cheap to add precisely because DEC-003 makes a schema change a reindex.

The name is `Unit`, not `Method`, because DEC-012's seam is only real if the
noun is: a Rust `fn` is the same record.

## DEC-015 — Flags do not touch the index; subcommands do

`--symbols` parses the file in front of it and `--status` reports on the
database; `index`, and later `dupes` / `search` / `summarize` / `similar`, are
verbs that fill or query the index. That is the rule a reader can predict
from, and it is why `--symbols` works in a directory contour has never seen —
which is the property that makes it usable as an editor outline.

The alternative (everything a subcommand) reads more uniform and hides the
distinction that actually matters to a caller: whether an answer depends on
state this machine may not have.

## DEC-016 — Two storage rules: derived is a cache, purchased is a record

DEC-003 calls the database "a cache of a pure function, not a system of
record", and a schema-version mismatch drops it and rebuilds. That is right for
everything derived from bytes this machine can read again — blobs, units,
structural hashes, the file map — where a rebuild costs seconds and removes an
entire class of migration bug.

It is **wrong for summaries.** A full fill of a large repo is hundreds of
dollars of LLM calls, and no amount of local reading reproduces it. Dropping
that because the extractor gained a macro would be indefensible. So the schema
has two halves with different rules:

- **Derived** (`blob`, `unit`, `checkout`, `file`) — governed by `user_version`,
  dropped and rebuilt on any mismatch, exactly as DEC-003 says.
- **Purchased** (`summary`) — its own version in a `meta` row, never dropped by
  a rebuild. If that version ever moves, the store **refuses to open** rather
  than guessing: a mismatch there is a migration someone has to write, or an
  admission of data loss someone has to make on purpose. The refusal is the
  point. The first time that number changes it must not silently destroy work
  that was paid for.

The two halves stay consistent by construction rather than by care: the summary
table is keyed entirely by content hashes and holds no foreign key into the
derived tables, so no rebuild can strand or orphan a row in it.

### The key-column asymmetry

DEC-014 says carry nothing speculatively, because a schema change is free. That
reasoning holds only for the derived half, and only for non-key columns. A
**key** column in the purchased half is different: adding one later re-keys
every stored row, and re-keying a summary means buying it again.

So `variant` (DEC-008's body-only vs comment-informed) and `level` (DEC-013's
container rollups) are in the primary key from the start, before either has a
second value. That is not a violation of DEC-014 but its complement — the same
question, "what does deferring this cost?", answered by a different arithmetic.

### Why server-side refusal fallbacks are off

Anthropic's API can re-run a refused request against a fallback model inside
the same call, and the guidance is to enable it by default. contour does not,
and the reason is this decision rather than laziness: **`model` is part of the
summary cache key.** A fallback returns an answer from a model other than the
one requested, so storing it under the requested key is a lie and storing it
under the responding model's key scatters one fill's purchases across indexes
nondeterministically — which the per-model coverage accounting would then
inherit. A refusal on source code should be vanishingly rare; the fill loop
records its category and moves on, like any other per-unit failure.

## DEC-017 — `super` makes the method's own name structure

`norm_hash` excludes a method's own name everywhere — that is what makes
`Widget#save` and `Gadget#persist` with the same body report as clones, and
what lets a rename never re-summarize. There is exactly one exception.

A body containing `super` gets the enclosing `def`'s name folded in, because
`super` dispatches *by* that name. Two byte-identical bodies ending in `super`
run different code. rails has four live instances: `LocalCache#increment` and
`#decrement`, `LogSubscriber#info` and `#error`, `CompareWithRange#===` and
`#include?`, and `V6_1::TableDefinition#change` and `#column` are each a pair
of identical wrappers whose only difference is which superclass method they
reach.

Reporting those as clones is worse than a harmless false positive: it offers a
consolidation that is **not available** without metaprogramming, so a reader
who acts on it wastes the trip. The eval set now carries all four as
`distinct`, which makes them the sharpest labels in it — if normalization ever
stops folding the name in, they fail first.

Cost: a `norm_hash` change, so a schema bump. Taken now deliberately, at the
one moment it is free — no purchased summary exists under the old key outside
fixtures, and after the first real fill this same change costs money to make.

## DEC-018 — Organic, incremental indexing

contour is useful at zero coverage and quietly better every session. That is
one principle, and four mechanisms compose into it:

1. **Cheap layers index eagerly.** Structure, structural hashes, signatures —
   a full `index` of rails is ~3 s and needs nothing but git.
2. **The identifier tier is free.** Every unit's humanized name, owner and
   parameters are embedded locally, so English search works on a fresh
   checkout with no LLM spend at all. It captures what code is *called*, not
   what it *does* — an honest floor, not a substitute.
3. **The expensive layer grazes.** Sessions using contour contribute summaries
   for the methods they read anyway, through the MCP `store_summary` tool. The
   index improves as a by-product of work already being done.
4. **Coverage-aware fills are the proactive complement.** A session starting
   substantive work in a thin scope fills that scope first — the highest-value
   target, because it is about to read most of it regardless.

This generalizes gqls's background auto-warm. gqls warms one schema's vectors
after a cold query; contour warms a corpus's *meaning* across sessions, and the
warming is done by the same agents that consume it.

### Why contributions are keyed apart

A contributed summary is written under the contributing model's id with
`via = mcp`, never merged into an API fill's keyspace (DEC-005's rule, applied
one layer up). The reason is calibration: **a heterogeneous pile of summaries
from whatever model happened to be running is the daily-use path, and a clean
Phase 1 threshold number still wants a uniform single-model fill.** Provenance
in the key is what keeps those two separable, so the eval can ask for one
without the other quietly contaminating it.

### Contributions are purchased, not derived

DEC-016's rules apply in full. A contributed summary cost a session's tokens
and a person's attention; that it was not billed in dollars does not make it
recomputable. It lives in the purchased half, survives every reindex, and is
gated on the way in — validated against the same schema the API path uses, and
refused rather than repaired, because a store that never forgets must not be
handed something quietly wrong.

### Deliberately not built yet

Session hooks that auto-contribute a budget per session, and scheduled or
nightly fills. Skill-level proactivity first: if instructing sessions to be
coverage-aware turns out to be insufficient, that is the evidence that
justifies infrastructure. Building both at once would leave us unable to say
which one worked.

## DEC-019 — Canonicality is agreeing signals, not a score

A duplicate group's next question is *which one do I keep*, and the answer is
not in the bodies — they are identical. It is outside them, in three signals
that are each cheap, local, and independently checkable.

**No composite.** Each signal is measured separately and names its own pick.
Nothing sums, weights, or normalizes a commit date against a call count into
one number, because that number would mean nothing and look like it means
something — DEC-010's rule, applied to a judgment with several inputs instead
of one. When every signal that spoke agrees, that agreement is the answer. When
they disagree there is **no pick**, and the disagreement is reported: it
usually means the older implementation was superseded and nobody deleted it,
which is precisely what a reader needs before consolidating.

One asymmetry, spelled into the output rather than hidden in a weight:
`git_age` and `references` are **deciding**, and `namespace_depth` is a
**tiebreak** consulted only when both are silent. Depth genuinely correlates
with generality and genuinely does so weakly; letting it disagree its way into
an abstention would make abstention the usual answer.

### What each signal actually measures

- **git_age** — the oldest *surviving* line of the body, via `git blame`. Not
  its introduction: a body nobody has edited dates to when it was written, and
  one reformatted last week dates to last week. rails' two `XmlMini#merge!`
  backends are a year apart in history and five days apart by this measurement,
  because both have been reformatted since. The note says "oldest surviving
  line" for that reason — a reader told "oldest" cannot discount it.
- **references** — `trekr --refs`, whose confirmed/possible tiering no grep can
  reproduce. Confirmed decides where anything has one; otherwise the comparison
  falls back to possible and **says which tier it used**, because idiomatic Ruby
  leaves most counts zero-confirmed and collapsing the tiers would report a
  weaker measurement under a stronger name.
- **namespace_depth** — `::` segments in the owner. The weak one, and a
  tiebreak only.

### Shelling out is the right call here

git owns history and trekr owns Ruby call resolution. A second, worse copy of
either inside contour would be the expensive kind of not-invented-here. The
price is that both can be absent, so both degrade to `unavailable` **with the
reason attached** — including trekr's own hint for making it available — and
never to a guess. A signal that cannot see one member of a set reports the
whole set unmeasured, rather than ranking the members it happened to see.

### Why it is opt-in, against the brief

The milestone asked for canonicality on *every* group. It is behind
`--canonical` instead, because the cost was measured rather than assumed: one
`git blame` costs ~1 s on rails (90k commits) and ~30 ms on a young repo, and
one `trekr --refs` ~2 s on an indexed rails. A full `dupes` of rails is 296
groups and 665 members, so annotating all of it is minutes against the 0.5 s
the report costs today. Making the cheap command expensive by default is the
wrong default; `contour dupes app/models --canonical` is seconds, and is the
shape somebody actually uses when they are about to consolidate something.

Reversible in one line if the owner disagrees, and every run prints what it
spent so the tradeoff stays auditable.

### Not on `similar`

`similar` returns neighbours across three tiers, and only the structural and
near ones are "the same implementation twice". Crowning a member of a list that
includes semantic neighbours would assert an equivalence nothing measured.
Canonicality ranks a set of candidates that are claimed to be interchangeable;
`dupes` produces such a set and `similar` does not.

## DEC-020 — A duplicate is a consolidation that would reduce complexity

Ratified by the owner, and it is a product definition rather than a labelling
convenience: **a pair is a duplicate iff consolidating it would reduce net
complexity.** `dupes` exists to offer a consolidation. A pair whose
consolidation needs metaprogramming or a behaviour change is not a finding, it
is a wasted trip — the tool would be proposing to increase complexity in the
name of removing duplication.

This is what DEC-017 was really applying. The `super` pairs are distinct not
because `super` is special but because consolidating them is impossible without
metaprogramming; the same reasoning makes rails'
`compatible_table_definition` copies distinct, and it makes `write_query?` a
duplicate despite looking identical to them, because one parameter consolidates
it. **The shape does not decide, the arithmetic does.**

Two consequences worth stating, since neither is obvious:

- The near tier is not a weaker exact tier. "Same except a few lines" is often
  the *best* consolidation candidate — a small edit for a real reduction — so
  the report ranks expected reduction per unit of effort, not similarity.
- The ordering is therefore the estimate itself: copies beyond the first, times
  the body's node count, discounted by the measured Jaccard. Displayed with its
  components, never as a bare score (DEC-010). Why nodes rather than lines is
  measured on rails and argued in `dupes::Group::saves_nodes`, where the choice
  lives; the labeling rule's worked examples are in `tests/eval/README.md`.
