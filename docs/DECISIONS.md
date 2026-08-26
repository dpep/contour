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
