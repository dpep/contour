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

## DEC-021 — Path signal is real; it lives at the file layer

**Clarifies DEC-003, which has now been re-fought twice.** "Layer 1 must never
see a path" is a statement about *which layer carries* path knowledge. It was
never a claim that paths carry no signal, and reading it as one is how four
separate findings went unaddressed.

Two halves, and only the first is a constraint:

- **Layer 1 stays a pure function of bytes.** Same bytes, same OID, same units,
  forever. That is what makes N worktrees, a branch switch, a rebase, and a
  no-op reindex free, and it is why a summary survives a file being renamed or
  moved. Keep it. Nothing below the `file` table may mention a path.
- **Layer 2 — the path→blob map — is exactly where path knowledge belongs**,
  and it has been sitting there unused. A blob at `db/migrate/…` and the same
  blob at `app/models/…` are the same parse and legitimately different
  *findings*, because the question "should I consolidate this" is a question
  about the place, not about the bytes.

The owner pressure-tested the constraint and it survives with this scope. So
the rule for the next cold reader: **if you find yourself wanting a path fact
in the extractor, you want it in the file layer instead.** That is not a
compromise, it is where it was always supposed to go.

### Siblings, not separate projects

Two things live at this seam and should be built as one piece of work:

1. **Path classes** (DEC-022), which is what four recorded findings were all
   asking for.
2. **The Rust free-function owner gap.** A top-level `fn` in `src/lang/go/mod.rs`
   has no owner, so five language plugins each contribute an identically-named
   `tests::find`. The module prefix that would disambiguate it is *in the path*,
   which is why the extractor cannot reach it and why the gap has stood since
   Phase 1.5. Same layer, same fix shape.

## DEC-022 — Path classes: ignore, include, or classify, and always disclose

`dupes`, `search` and `similar` currently treat every indexable file as one
population. Four findings say they are not:

| finding | corpus | what it costs today |
| ------- | ------ | ------------------- |
| schema migrations collide byte-for-byte | discourse | exact precision 0.93 → 0.73 |
| test code outranks the implementation it tests | berater | a spec helper above `Berater::Limiter#limit` |
| `.rb` fixture corpora index as real units | trekr, rwr | a sibling repo's dupes report is mostly fixtures |
| unqualified constants resolve per nesting | rails | a 6-line false collision no floor can fence |

**The defaults, ruled by the owner:**

- **Migrations — ignored entirely.** Under DEC-020 they are not duplicates at
  all: migration history is frozen, so consolidating one is not a consolidation,
  it is a rewrite of the past.
- **Generated and vendored code — ignored.** Nobody consolidates it; it is
  regenerated or re-copied.
- **Tests and fixtures — INCLUDED, and classified.** Not ignored: duplication in
  shared examples and helpers is real maintenance signal. But it is a *different
  population*, so it is tagged and ranked separately from app code rather than
  interleaved with it. This is the ruling most likely to be mistaken for the
  easy one — "ignore tests" is the tempting default and it is wrong.

**Every default is disclosed and overridable.** A report that silently withheld
groups would be a worse failure than the one this fixes: `N group(s) in ignored
paths withheld` on every run, plus the flag that shows them. Healthy defaults,
a config for a repo whose layout differs, options for the one-off.

Classification attaches at the **file** layer (DEC-021), never the blob layer.
The same bytes vendored into one repo and authored in another are one parse and
two classifications, which is precisely the property that makes this correct.

### As built (M11a), and ruled

**A class is a pure function of the path string**, never of the bytes.
`scan::language` already made that call one layer up — sniffing content would
mean reading every file in the repo to answer a question the layout answers for
free — and it is what keeps reclassification free: no reindex, because nothing
below the `file` table ever learns about it. The cost is that a fact absent from
the path is invisible here, and a Rust `#[cfg(test)] mod tests` is the standing
example: it sits in an app file and classifies as `app`.

**Six classes**: `app`, `test`, `fixture`, `migration`, `generated`,
`vendored`. Each of the five named ones exists because a corpus produced a
finding that needed it; the conventions behind them are deliberately thin,
because a rule that withholds real code is worse than one that misses.

**A group is withheld only when *every* copy sits in an ignored path.** One
whose copies disagree reports as `mixed` — not a class, since a file has one
class and a group of files need not — and ranks with app code. A body in both
`vendor/` and `app/` is a finding about the app copy, and contour's own
`ParamKind` pair is the worked example: `core.rs` against the vendored
`ruby/facts.rs` stays visible and is reframed as spanning the vendor seam,
which is exactly why that duplication exists.

**Overrides, in one mechanism.** `.contour.toml` at the checkout root, a
`[paths]` table of class name → path prefixes, believed over the conventions:

```toml
[paths]
app = ["db/migrate"]      # these really are consolidatable here
test = ["engines/billing/spec"]
```

Rules are **prefixes, not globs** — matched by the same `paths::under` a
`SCOPE` uses, so there is one path language in the tool rather than two that
almost agree, and a rule containing `*` is refused with that reason rather than
silently matching nothing. Longest prefix wins, so the file needs no order a
reader could get wrong. An unknown class name, a duplicate prefix, or an
unknown table fails the run: a config half-applied would withhold what somebody
wrote it to keep. Deliberately **no per-class policy table** — reclassifying to
`app` already covers every recorded need, and two mechanisms for one job is the
kind of redundancy that rots.

**One-off**: `--include-ignored` (CLI) / `include_ignored` (MCP), which reports
every class and reports nothing as withheld, because nothing was.

**`search` discounts rather than drops** a hit outside the app population, at
`search::NON_APP_DISCOUNT`, disclosed as `discount` on every answer with the
`class` on every hit. That constant is **not calibrated** — DEC-011 says a
ranking constant comes from the eval set, and no labeled query expects a test
method. What it is measured against is regression: no labeled query on any of
the seven sets changes rank, and both known live cases flip.

## DEC-023 — What a summary is worth against a name

**Status: ratified at the M11b review**, both constants as built, with the
stated-cost paragraph amended to say what the bias is *for* as well as what it
costs.

The field trial asked for a summary it had contributed and got it fifth. The
mechanism is not the embedder: RRF fuses two *rankings* and discards the cosine,
so a summary hit at 0.44 and an identifier hit at 0.20 two places above it were
worth 0.0276 and 0.0277 — indistinguishable. The semantic half had no way to say
that one of those matches was evidence of a different kind.

**The ruling this proposes:** the semantic half's weight depends on which vector
answered — `SUMMARY_WEIGHT` 1.0, `IDENTIFIER_WEIGHT` 0.7. The identifier weight
is the old single constant, unchanged, so a corpus with no summaries ranks
exactly as it did and one at complete coverage is only rescaled. The weights
differ **only where the tiers meet**, which is both where the old single weight
was wrong and the state DEC-018's grazing bargain is about: if contributing a
summary does not visibly change what comes back, the flywheel has no flywheel.

**Not calibrated, and it cannot be**, for the same reason as DEC-011's other
uncalibrated constant: none of the seven sets is both partly summarized and
labeled. What it is measured against is regression — fixture (complete coverage)
holds at 0.27/0.55, rails and discourse (no coverage) are identical — plus the
live repro, which flips, and an e2e that fails if the weights are equalized.

**The cost and the point, which are the same sentence read twice.** At partial
coverage a summarized unit is systematically favoured over an unsummarized one
that matches about as well.

- *The cost:* a warming corpus is biased toward its covered half, and a unit
  nobody has summarized is harder to find than it was on the same query
  yesterday. `tiers` on every answer is what lets a reader see this.
- *The point:* under DEC-018 a summary **is** strictly better evidence than a
  name — it says what the code does rather than what someone called it — so the
  bias is half-intended. A contribution that changes nothing about what comes
  back is a contribution nobody makes twice; the visible payoff is what makes
  grazing rational.

If the bias ever costs more than the flywheel gains, the fix is one constant and
this entry says which. Calibrate it properly when a partly-summarized labeled
set exists.

### Postscript (M11c): the 0.70 near threshold, re-ruled on per-population evidence

M11c's re-audit of rails' near labels produced an alarming headline — an
unbiased sample of what the tier ships at 0.70 was **2 real in 20** — and that
number turned out to be a *population artifact*, not a threshold problem. Path
classes (DEC-022) already section test duplication away from app code, so the
number that decides a threshold is per-population precision, not blended.

The app population is small enough to need no sampling at all. Of the **230**
near groups rails ships at 0.70, **22 are app code** (205 test, 3 mixed), and
all 22 were read:

| band | real | precision |
| ---- | ---- | --------- |
| app, >= 0.80 | 8 of 11 | 0.73 |
| app, 0.70-0.80 | 10 of 11 | **0.91** |
| app, >= 0.70 | 18 of 22 | **0.82** |

**0.70 stands, and the earlier ruling was right for a reason it did not have.**
App precision at the shipped threshold is 0.82. Raising to 0.80 would *lower*
it to 0.73 while discarding 10 of the 18 real app findings — because the band
nearest the threshold is the cleaner one. The three false positives above 0.80
are deprecation accessor pairs (`X` and `X=` whose bodies are identical), where
the consolidation needs metaprogramming and the labeling rule therefore says it
is not a finding.

**A second threshold for test-class groups was measured and rejected.** The
tempting move was to raise the bar where 19 of 20 of the junk lives. Sampled
test-population precision is **0.14 above 0.80 against 0.08 below** — the junk
is junk at every threshold, because sibling test methods are near-identical *by
construction*: the shape they share is the harness, not the behaviour. A second
constant would buy six points and add a number to explain forever. Sectioning
plus payoff ranking already keeps that population out of the reader's way, and
that is the mechanism that earns its keep.

**The blended figure is retired.** rails' printed near precision mixes app and
test populations and answers no question anybody has; the split above is what
should be quoted.

## DEC-024 — The constant-scope caveat, as built

**Status: built at M11c** against the M11 ruling recorded in `docs/PLAN.md`
("Context-dependent constants — caveat, never fold"). The ruling stands; this
records the one place the implementation is **narrower than what was approved**,
because the difference is measurable and a reader should not have to rediscover
it.

The approved row was "caveat where such a constant is defined under more than
one nesting", measured at 54 of rails' 296 exact groups. As built it asks a
stricter question — whether the copies **reach different definitions**, by
Ruby's own lexical rule — and flags **22 of 292 (8%)**.

The stricter rule is not a refinement for its own sake. rails defines `Array`
inside `ActiveRecord::ConnectionAdapters::PostgreSQL::OID` as well as at the top
level, so the approved rule flags every pair of copies that so much as mentions
`Array`: 44 groups, of which roughly half are pairs that both plainly reach
`::Array` and are perfectly interchangeable. A caveat that is wrong half the
time is one a reader learns to skip, which costs more than not having it. What
survives the stricter rule is specific enough to act on — `TableDefinition`
across three version modules, `READ_QUERY` across three adapters,
`DATE_FORMATS` across `Date` and `Time`.

**Lexical only.** A definition reached through an ancestor chain is a tree-layer
question contour does not answer (DEC-013), and guessing at one would be a guess
wearing a warning's clothes. The caveat therefore *understates*: it flags what
it can prove differs and stays quiet otherwise.

**On by default, unlike canonicality.** The cost is one `rq` call per distinct
constant, deduplicated and run in parallel — 68 calls and 0.6s of a 1.8s run
across all of rails. Canonicality is opt-in because it costs a `git blame` per
body; this does not, and a caveat nobody asked for is exactly the kind a reader
needs. Where `rq` is absent the run says the candidates went **unchecked**
rather than reporting nothing, because silence would read as "safe to
consolidate" — the one direction this feature must never fail in.

**What a third corpus says it is actually worth.** mastodon was labeled
independently and the caveat had never been run against it. It flags **3 of 60
groups**, and the result is the useful counterweight to rails':

| flagged constant | the labeller's verdict on the pair |
| ---------------- | ---------------------------------- |
| `ROWS_PROCESSING_LIMIT` | **duplicate** — "20_000 in both classes; one shared concern consolidates" |
| `IGNORED_PARAMS` | **duplicate** — "a base filter class is the consolidation" |
| `CACHE_TTL` | unlabeled |

Two of three flags sit on consolidations a human judged available. That is not
the caveat being wrong: it flags that the copies *reach different definitions*,
which they do, and whether that blocks the merge depends on whether the values
agree — which the tool cannot know without evaluating them, and does not claim
to. The labeller's own note is the proof it points at the right thing: they
checked both values and found them equal, which is exactly the check the basis
line asks for.

So the honest reading is that **this is a question, not a verdict**, and the
report's wording ("check that they resolve to the same thing before
consolidating") is the claim it can support. On rails it asked about genuine
blockers; on mastodon it asked twice and the answer was "fine". A reader loses
a minute; the alternative is a silently offered consolidation that does not
exist.

**The refinement this measurement earns, if the noise ever costs more than it
saves:** compare the definitions' *values*, not just their locations, and stay
quiet when they agree. `rq` already returns each definition's file and line, so
the text is one read away. Not built — a literal text comparison is fragile
(computed values, multi-line literals), and on this evidence the noise is three
groups in sixty.

## DEC-025 — A resident MCP server becomes the binary that replaced it

A `contour mcp` process outlives the binary it was launched from. Install a new
contour mid-session and the server is instantly the old one; the moment anything
brings the shared database up to the new derived schema, `Store::init`'s guard
fires — correctly, DEC-016's "an older binary must not drop a newer database" —
and **every** tool that reads the index fails for the rest of the session. Two
M11 field trials lost their whole grazing budget to that, twice, with no
in-session recovery. One trial contributed nothing at all; the other hand-rolled
a Python stdio client against a fresh `contour mcp` to get around it.

That makes it the flywheel's single point of failure: DEC-018 says the expensive
layer fills by grazing, and grazing happens through this one process.

**As built: the server becomes the new build.** It stamps the binary it was
launched from (path, size, mtime), restats after each answer, and `exec`s the
replacement in place when the stamp moves. `exec` keeps the pid and the file
descriptors, so the client's pipes survive and it never learns that the program
on the other end was replaced.

Three things make this small enough to be worth it:

- **Almost nothing needs carrying across.** `handle_line` holds no session state
  — a `tools/call` is answered the same way with or without a preceding
  `initialize` — so there is no handshake to replay. That property is now pinned
  by a test, because the restart depends on it rather than merely benefiting
  from it. The one exception is the client's *tool list*, which it fetched from
  the build that has just been replaced and has no way to know to re-fetch:
  nothing happened, as far as it can see. So one environment variable crosses
  the exec, and the new process opens by sending
  `notifications/tools/list_changed`. Answering calls correctly while describing
  them wrongly would be a half-heal, and `initialize` now declares
  `listChanged` because it is true.
- **The inode is what says the binary moved.** An installer stages a new file
  and renames it over the path, which always changes the inode but *not*
  necessarily the mtime — a clone on APFS carries the original's timestamps
  across, which is how the first version of this, watching size and mtime, sat
  through an upgrade in a test and noticed nothing. Size and mtime are kept
  beside the inode for a rewrite in place, which is not how anything installs
  but is how a build script might.
- **The trigger is the binary, not the failure.** Skew is the *symptom*; the
  upgrade is the cause, and statting a path is free where opening a database is
  not. Restarting on the skew instead would loop forever in the one case exec
  cannot fix — a database moved ahead by some *other* contour on the machine,
  where the installed binary really is too old and "upgrade contour" is the
  right answer. So: exec when the binary changed, report when it did not.
- **The praised error text is unchanged.** A failed tool call gains one sentence
  — and only when a newer contour is actually on disk — saying the server is
  restarting into it and the session does not need restarting. That is the fact
  the message could not otherwise know: "upgrade contour" is stale advice to
  somebody who just did.

### The known edge, and the shape that removes it

The `exec` happens at the one moment the process owes nobody an answer: a reply
has just been flushed and the next request has not been read. A client that
*pipelined* a batch can still have siblings sitting in a buffer we cannot see,
and those are lost. It is bounded — those calls were already failing — but a
lost request is a hang where a failed one is an error, which is worse.

Removing it properly means the Envoy hot-restarter shape: a thin, stable parent
that owns stdio and proxies to a restartable child. That is genuinely more
robust and it is a second component, a second process, and a protocol between
them. Recorded as the fallback if self-exec meets a platform edge.

Two edges are known, both about *how* an installer writes:

- **An installer must rename over the path, not truncate it.** Truncating the
  running binary in place kills the process before it can exec anything. `cargo
  install` and `brew` both stage and rename, so this is not a limitation in
  practice — it is how a live run of this feature first failed, by simulating an
  install with a plain copy, and both tests now model the rename.
- **An install that replaces a *symlink* rather than the file `current_exe`
  resolves to** leaves the stamp unmoved, so nothing is detected. A Homebrew
  upgrade relinks the Cellar; `cargo install` writes the file. Measured on the
  `cargo install` path, which is how contour is installed today; the day it
  ships through the tap, this is the thing to re-measure.

### The other way to define this out of existence — APPROVED AND BUILT

**Version the derived database in its filename.** DEC-016 already split the
schema into a derived half that may be dropped and a purchased half that may
not; this puts them in two files and names one of them for its version. A v10
binary opens `contour-derived-v10.db` and a v11 binary opens
`contour-derived-v11.db`, so neither can see, drop, or be refused by the other's
— **skew stops being a state that can arise**, and the "two contours take turns
wiping each other's index" hazard `Store::init` guarded against goes with it.

The restart above is not thereby redundant. It still carries a session across an
upgrade without a client noticing, and it is what makes a *new* build's tool list
reach a client that already fetched the old one.

**The owner's constraint, and it is the load-bearing half:** the purchased store
stays a **single unversioned file** that every contour version reaches through
DEC-016's migration discipline. Splitting it per version would orphan paid work
on every upgrade, which is the one outcome that decision exists to prevent. A
test asserts exactly this — delete the derived file, which is what a version bump
amounts to, and the summaries are still there.

As built: the derived file is `main` and the purchased one is `ATTACH`ed as
`purchased`, because the derived half is what nearly every query touches.
`$CONTOUR_DB` still names the purchased database — the file a person configures,
`--status` prints, and nothing may ever drop.

**The legacy tables are left alone, on purpose.** A database written before this
holds both halves in one file; opening it now builds a fresh derived file beside
it and attaches the old one for its summaries, which are untouched (measured: 45
of 45 on a 79 MB store). The dead derived tables in it are *not* dropped, and the
reason is the change's own premise — an older contour on the same machine is
still using them, and reclaiming the space would be one binary wiping another's
index, which is precisely what this stops. An explicit cleanup command can offer
the space back later; a silent one would contradict the feature.

## DEC-026 — A Rust unit's owner includes the module its file declares

**Approved by the owner as a uniquely-cheap-moment argument**, the same shape as
DEC-017's: the fix costs re-keying Rust summaries, Rust coverage was 42
summaries, and that number only grows.

DEC-021 named this as path classes' sibling and it had stood since Phase 1.5. A
top-level Rust `fn` has no lexical owner, so rq's five language plugins each
contributed an identically-named `tests::find`, and nothing downstream could
tell them apart. By M12a it was costing three surfaces:

| surface | what it cost |
| ------- | ------------ |
| `similar` | refused the name as ambiguous, listing locations instead of answering |
| `search` | ranked five copies of one answer, and scored one lexical match five times |
| `store_summary` | refused `contributed::accept` — the name a reader of the source would write — because contour knew that unit as bare `accept`. A wrong name here costs a session's tokens, not a query. |

**As built.** `paths::rust_module` derives the module from the path string, and
`paths::qualify` composes it onto the unit's owner at the two file-layer
boundaries: reading a unit out of the store, and outlining a file live. Layer 1
is untouched — the `unit` table still holds the bare lexical owner, so the same
blob still yields the same rows wherever it sits, and **no reindex is needed**.
That is DEC-021's rule applied rather than bent: the module prefix was never
missing from the extractor by oversight, it was in the path all along.

Three properties worth stating, because each was a decision:

- **One rule, no special case.** Every Rust unit gets the prefix, not only the
  ownerless ones. "Prefix unless it already has an owner" is a rule a reader
  cannot predict from, and it leaves two `Registry::new` in different modules
  colliding. `summary::anthropic::Anthropic::from_env` is long; it is also what
  Rust itself would call that function, minus the crate name.
- **A module path is made of module names.** Only the trailing run of
  directories that could *be* a module name is used, so a fixture tree at
  `tests/testbed/006-rust-names/app.rs` yields `app` and not
  `testbed::006-rust-names::app`. Without that the answer depended on how far up
  the caller was standing when they named the file — `--symbols` from a repo
  root and `--symbols` from inside it would disagree, and the id a session looks
  up is the id it must contribute under.
- **`mod.rs`, `lib.rs` and `main.rs` name no module of their own**, so
  `src/store/mod.rs` is `store` and a free function in `src/lib.rs` keeps its
  bare name.

### What it cost, measured

**42 contributed Rust summaries became unreachable** — contour's own 24 and rq's
18 — because `ctx_hash` covers the context the prompt renders and the owner is
part of it (DEC-003). Nothing was deleted: the rows are still in the purchased
half, keyed under the owner they were bought with, and `pending` now re-offers
those units so the flywheel refills them as a by-product of the next session's
reading. Ruby was unaffected, having never had this gap.

A rekey migration is possible and was **not** written. It would have to
recompute both the old and new `ctx_hash` for every unit in every checkout this
machine happens to have indexed, which makes the contents of the purchased half
depend on which repositories are checked out — a machine-dependent result in the
one table DEC-016 says must never be guessed at. Re-grazing is minutes of work
and cannot be wrong.

**The eval numbers** (seven Rust sets, scratch database, same binary on both
sides): top1 unchanged at 2/21, top5 7/21 → 8/21, every duplicate-tier number
identical, no set worse. Fifty-seven labels were relabelled by measuring each id
against its checkout; two rows turned out to have been **ambiguous all along**,
which is written up in `tests/eval/README.md` because it is a finding about the
eval and not only about this change.

## DEC-027 — Both halves of the fusion state their own evidence

**Status: built at M12b**, and it is DEC-023's ruling applied to the other half
rather than a new idea. That entry says RRF consumes a rank and discards the
cosine, so the semantic half has to *say* which vector answered or the fusion
cannot tell a summary from a name. The lexical half has exactly the same
problem and had no such voice: `lexical_score` returns how much of the query a
name accounts for, and the fusion added `1.0 / (K + rank)` whatever that number
was.

So a name sharing one filler word with a ten-word question was worth what a name
that answered the question outright was worth. With `RRF_K` at 60 the two stay
within a factor of two for about 135 places, which is why reordering the lexical
list could not fix it and why the shipped fix multiplies instead:

```
lexical:   score  / (K + rank)      where score is lexical_score, 0 to 1
semantic:  weight / (K + rank)      where weight is SUMMARY_ or IDENTIFIER_WEIGHT
```

**No constant was added.** The semantic half's weight names a tier; the lexical
half's *is the measurement*, which the ranker was already computing. A full name
match still contributes exactly `1.0 / (K + rank)`, so the top of the lexical
ranking is bit-for-bit what it was and only weak evidence is repriced as weak.

Measured on all eleven labeled query sets — every number and the two rejected
variants are in `docs/PLAN.md`. **Top-1 21 → 30 and top-5 34 → 57 of 195
queries**, `found` identical everywhere (this moves ranks, never recall), and no
set worse anywhere. The two most convincing rows are the extremes: `fixture`,
12/22 → 21/22 at top-5, the **only set with complete summary coverage** and so
the state DEC-018's flywheel is walking every corpus toward; and `rails`, 10/77
→ 21/77, the largest corpus and therefore the one with the most long names able
to share a word with a question by accident.

**What follows from it, and needs the owner:** the milestone was briefed to fix
this with IDF over the identifier corpus, and IDF was built, measured, and
**not shipped** — see `docs/PLAN.md` for the numbers and for why the natural
phrasings in `queries_natural.tsv` are the labels that could still settle it.
Retiring a recorded direction on a measurement is the implementer's job;
declaring it dead is not.

**The lexical measurement is now disclosed on every hit** (`lexical` in JSON,
`name 0.14` in human output), for the reason DEC-010 gives: the half used to be
a predicate, where `how: both` said everything there was to say, and it is now
graded. `both` on a name that matched one filler word and `both` on a name that
answered the whole query are the same word for very different evidence, and a
reader could not otherwise tell which one they were looking at.

## DEC-028 — A unit knows who may call it

**Approved by the owner at the M12b checkpoint**, on the argument that this
completes the "what a caller sees" set `Unit` already carries rather than
reopening DEC-014's deferral of containers.

`core::Unit` gains `visibility`: `public | protected | private`. The Ruby
extractor has always computed it — visibility stacks, `module_function`,
`private_class_method` — and dropped it at the `ruby::units` seam; the Rust
extractor now reads `visibility_modifier`, where `pub` is public, a bare `fn`
is private, and `pub(crate)` / `pub(super)` / `pub(in path)` are **protected**,
which is the honest bucket for "visible past here, not to everyone".

**Three values, not a flag**, because two of them are not the same fact in
either language and collapsing them would lose one a reader wants. That is also
why this is `core::Visibility` and not `bool is_public`: the predicate the
ranking wants is one question this answers, not the whole of what it says.

### Carried because a question needs it

DEC-014 says a record that carries a fact invites a query that depends on it,
and that is the rule this obeys rather than an exception to it. The question
arrived first, from the M12b census: mastodon's entry points are named for the
*protocol* — `call`, `to_s`, `get`, `use`, `hydrate`, `refresh` — and their
private helpers are named for the *behaviour*, so the container out-ranks its
own entry point on 6 of 21 labeled queries. **A container whose only public
method is that method is the one contour can nominate, and nothing but
visibility can tell that method from its helpers.** Nine of twenty labeled
answers are their container's sole public method, and those nine are the broken
cases.

`singleton` and `params` are here for exactly this reason already. Anything a
caller sees is a fact about the callable; anything about the container is not,
and stays out (Phase 3, DEC-013).

### It must never enter the summarizer prompt

**The constraint the owner asked to be written down.** `ctx_hash` covers exactly
what the prompt renders (DEC-003), so rendering visibility would re-key every
summary in the purchased half — the table DEC-016 says must never be dropped,
and a bill DEC-026 has already paid once at 42 summaries. It would be a
defensible thing to tell a model, and it is not worth that.

The type system already enforces this: `summary::Context` is a separate struct
holding exactly the fields `render` writes, so a `Unit` field is invisible to
the prompt until somebody adds it there twice. A test pins it anyway
(`a_fact_the_prompt_never_says_is_not_in_the_key`), because the cost of finding
out later is measured in money.

### Cost

A derived-half schema bump, 11 → 12, which is a reindex and nothing else —
DEC-025's per-version derived file means a v11 contour and a v12 contour do not
even see each other's. No summary is re-keyed, no vector is re-embedded (the
embedded text is unchanged), and no ranking moves: this commit carries the fact
and reads it nowhere. Measured rather than asserted — nine labeled sets on a
fresh v12 database, every top-1, top-5 and found figure identical.

### Two things it fixed on the way

- **`def initialize` extracted as public.** Ruby makes `initialize` private
  whatever the source says, and contour modelled only the lexical
  `private`/`public` stack. A Ruby fact rather than framework knowledge, and it
  is load-bearing here: `StatusCacheHydrator` has one public method once
  `initialize` is read correctly, and that method is the labeled answer.
- **`--symbols` now says `[private]`** on anything that is not public, and only
  then — an outline is mostly public methods, and a word that never varies is a
  word nobody reads. The same rule the `class` tag follows on a search hit.
  `visibility` is on every unit in JSON.

**A trait method carries no modifier and is left as the extractor sees it.**
Knowing it is as public as its trait needs the trait's own visibility, which the
walk does not carry down. The nomination rule abstains on a container with no
single public unit, so an honest silence costs less than a guess.

## DEC-029 — A container answers for its one public unit

**Approved by the owner at the M12b checkpoint**, over the alternative of
grouping results by container, on the argument that a nomination resolves to a
*unit* and can therefore be scored by the eval where a presentation change
cannot. That argument then did its job: the mechanism was measured twice and
changed shape both times.

### The finding

An entry point is named for the **protocol** it implements — `call`, `to_s`,
`get`, `use`, `hydrate`, `refresh` — and its private helpers are named for the
**behaviour**. So on mastodon the container out-ranks its own entry point on 6
of 21 labeled queries: `BackupService#call` sat at rank 40 while
`#build_archive!` was first, and `StatusCacheHydrator#hydrate` did not rank at
all while `#fill_status_payload` was first. Some unit of the right container
was in the top five for 7 of 21 queries where the answer itself made 4.

The meaning is in the container; the thing a caller wants is the one part of it
that carries none of the meaning.

### As built

A **query-time view**, and that is the load-bearing half. DEC-013's thesis
(coarser units, not blurrier ones) arrives early; its record model does not. A
container here is the units already in hand, grouped by lexical owner — nothing
stored, no cache key, no schema, no reindex — and the answer is still a list of
units, which is the one noun contour has. Phase 3 can still build the real
thing.

The container is ranked by the cosine of the query against its **centroid**, the
running mean of its members' vectors (ae's incremental-mean trick, which Phase 2
lists), and that rank goes to the unit it nominates:

```
container:  cosine * IDENTIFIER_WEIGHT / (K + rank)
```

`IDENTIFIER_WEIGHT` is DEC-023's existing floor, taken even where every member
is summarized, because a centroid is a mean of vectors none of which belongs to
the unit being nominated. The `cosine` is there for DEC-027's reason, and it is
what made this shippable: at the unit level a tier says what *kind* of evidence
answered and the rank carries the rest, but every centroid is the same kind of
evidence, so the cosine is the only thing separating one container's claim from
another's.

### The rule: a container's sole public unit, and it abstains

One rule, no framework knowledge. A container with exactly one public unit *is*
that unit as far as a caller is concerned — the service-object shape stated
structurally rather than by knowing what Rails calls a service. Anything else
nominates nobody, which is DEC-010's ambiguity-has-its-own-status rather than a
pick between candidates.

Three qualifications, each of which cost a measurement to find:

- **A macro-generated accessor is not a front door.** `via.is_some()` means
  declared rather than written, with no body to be an entry point of. Rails
  classes carry `attr_reader` routinely, and without this `BackupService` had
  three public units and nominated nobody — the first build silently did nothing
  for the exact case it was written for.
- **Two units minimum**, or a container is a second vote for a unit already
  being ranked on its own name and its own vector.
- **`initialize` is private** (DEC-028), which is what leaves
  `StatusCacheHydrator` with one public method at all.

The rule bounds its own blast radius, which is why it needs no threshold: a
large class matches many queries *and* has many public methods, so it never
speaks. `FeedManager` has twenty and stays silent.

### What was measured, including what was dropped

| | 7 Rust | fixture | mastodon | discourse | rails | total t1/t5 |
| --- | --- | --- | --- | --- | --- | --- |
| before nomination | 6/8 | 10/21 | 3/4 | 0/3 | 11/21 | 30/57 |
| flat `IDENTIFIER_WEIGHT` | 3/6 | 10/21 | 3/6 | — | — | — |
| **shipped (`cosine * IDENTIFIER_WEIGHT`)** | 6/9 | 10/21 | **4/6** | 0/2 | 11/21 | **30/59** |

**Read the whole table, not the first three columns.** On the nine sets that
run in minutes this looked like top-5 33 → 36 with top-1 level; across all
eleven, 195 queries, it is **top-5 57 → 59 and top-1 unchanged at 30**. rails —
the largest set — does not move at all, and discourse loses one. The gain is
mastodon's two plus gqls's one, against discourse's one.

So this is a **targeted fix that pays on the population it was built for and is
about neutral elsewhere**, not a general ranking improvement, and the honest
argument for keeping it is that the failure it addresses is a product failure a
number does not capture: an entry point nobody can reach is worse than a rank.
Every nomination is disclosed, so the day it looks wrong it will look wrong out
loud. If the natural-phrasing band (M12b item 5) does not improve on this, the
weight is one line and this entry says which.

**A container-lexical half was built and dropped**, and the reason generalizes:
`lexical_score` over a container's text (owner plus every member's name) is not
the same measurement as `lexical_score` over a unit's id. Matching one query
word in a thirty-word document is easy where matching one in a three-word name
is not, so the same number meant something weaker and swept the top of every
ranking. And once the container's text is cut back to its name, it says nothing
the nominee's own id does not already contain — every member's id begins with
it. The centroid is where a container's value actually is.

### The known cost, named rather than fenced

The rule generalizes to Rust modules, where it is a coarser claim than in Ruby:
`commands::locate` has one `pub fn run`, so it nominates the command's entry
point for any query its module matches. On navi, "remember where a file was
moved to" now returns `commands::locate::run` above `commands::locate_file` —
the module really is what the query is about, and the specific function is still
the better answer. It costs one top-1 across the twenty-one Rust queries and the
aggregate is still positive, so it is recorded rather than special-cased: a
language test here would be the first of a kind this codebase has managed to
avoid.

The deeper version of the same tension: a container's centroid includes its
nominee's competitors, so a container can ride on the vector of the very member
it then displaces. Worth revisiting if the natural-phrasing band (M12b item 5)
makes it look worse than it does here.

### Grouping is not dead, it is the other half

The owner ruled nomination over grouping because only one of them is scoreable,
not because grouping is wrong — the census says the information is *already* in
the result list, in a member of the right container. A grouped rendering of
nominated results is the likely eventual UI, and it is a human-output change
that can land whenever somebody wants it.

## DEC-030 — Scope is the cost control, and every command that can take one does

A field trial on a monorepo of ~2M indexed units — 40× the "~50k records,
revisit at monorepo scale" non-goal in the plan — found `similar` was the one
query surface with no `scope`. `search` and `dupes` both take one; `similar`
took a *path*, documented as locating the checkout and nothing else, so there
was no way to say "look here" and every call had to hold a vector for every
unit in the repository. Unscoped `similar` and `dupes` on that corpus ran 20+
minutes on ~10 cores with no way to bound them.

**The second positional of `similar` is now a scope, exactly as it is for
`search` and `dupes`**, and the MCP tool takes a `scope` property with the one
shared description all three now render from. One noun, one meaning, three
commands.

Three things fall out of it that are worth stating:

- **The scope narrows the answer, never the question.** The unit asked about is
  resolved against the whole checkout first and then carried into the scope, so
  `similar Widget#save app/billing` asks "is there anything like this in
  billing" rather than failing with "no such unit". The special case that would
  otherwise need handling does not arise.
- **The working directory is a scope, and now says so.** This was already true
  of `search`, `dupes`, `pending` and `summarize` — `scoped(None)` resolves `.`
  against the repo root — and it was true silently, while two of those
  commands' help text claimed they defaulted to the whole checkout. `similar`
  now behaves the same way, and `Neighbors` carries a `scope` field so an
  answer from one directory cannot read as an answer from a thin corpus
  (DEC-010). The stale help text is corrected in the same change.
- **`index` deliberately gets no scope.** `index` writes the checkout's whole
  file map (`store.write(root, files, …)`); a partial one would silently delete
  every path outside the scope from that checkout's view, which is a corrupt
  index rather than a cheaper one. It is also the wrong lever: indexing is
  blob-keyed and re-parses only what this machine has never seen, so the second
  run over a monorepo is a git call and a fold. The expensive layer is
  embedding, and that is what `scope` bounds.

Scoping is a **narrowing of the answer, not of the truth** — a duplicate
outside the scope is still a duplicate. That is the same bargain `dupes` has
always struck, and the reason both surfaces disclose the scope they searched.

## DEC-031 — The server reads on one thread and works on others

Three of the five findings in the first monorepo field report are the same
sentence: **a loop that blocks on the work it is doing cannot notice
anything.** A `symbols` call, documented as needing no index at all, sat for
fifteen minutes behind two unscoped heavy calls. An abandoned call went on
burning every core until the process was killed by hand. Neither is a
performance problem; both are the shape of `serve`.

**As built.** One thread reads stdin and hands each request to a channel; four
workers take them and answer, writing whole lines under a mutex because two
interleaved responses are one corrupt line. Responses come back in whatever
order they finish, which JSON-RPC allows and every client already handles by
matching on `id`. `notifications/cancelled` is acted on by the *reader*, which
is the whole point of it being free: it sets an `AtomicBool` the running
request watches.

### Why four workers, and why the bound is memory

CPU needs no bounding here. The heavy work is one shared rayon pool whatever
number of requests submit to it, so N workers do not become N pools. What does
not share is **memory**: an unscoped query holds a vector for every unit in the
checkout, measured at 757 MB peak for 132k units, so four of them on a monorepo
is gigabytes. Four is enough that the cheap call a session actually issues
never waits, and low enough that the box survives someone asking four expensive
questions at once.

**Postscript, after DEC-033: the sentence above is now about the worst case
rather than every case.** A *scoped* query holds a vector for every unit in its
scope, not in the checkout — that is what DEC-033 changed, and a session on a
monorepo is scoping, because DEC-032 refuses it otherwise. Four workers stands
unchanged: the bound has to survive four callers who each named the checkout
root, and that is exactly the case the 757 MB was measured on.

### The cancellation flag is ambient, and that is a considered trade

`crate::cancel` puts the flag in a thread-local rather than in a parameter. The
alternative is a `&Cancel` on `search`, `similar`, `dupes::find`,
`dupes::find_near`, `near::pairs`, `vectors_for` and `embed_all` — six of which
have a CLI caller with nothing to pass but a token that is never set, and every
function added later would have to remember it. That is a parameter costing
about as much as it hides.

The price is one sharp edge, and it is documented at the module and at the one
place it bites: **a rayon worker is a different thread**, so a parallel loop
must take the token on its own thread and let the closure capture it. Reading
it inside the closure gets a fresh, never-cancelled one and the run does not
stop. `embed::embed_all` is the worked example.

Three places check it, chosen because they are where the time goes (see
PLAN.md's scale measurements): the embedding map, `near::pairs`' bucket walk,
and the write that would otherwise store what a cancelled embedding did not
produce. `embed_all` cannot break out of a rayon map, so a cancelled run turns
every remaining item into a no-op and drains in about the time one embedding
takes; the caller treats the result as void.

### The hang-up is the cancellation that always arrives

Whether a given client sends `notifications/cancelled` is not something this
server can rely on — the notification is in the MCP spec, and support for it
across clients is uneven. **Closing stdin is not optional**: when the client
goes away the reader sees EOF, cancels everything outstanding, and joins. That
alone answers the field report's fourth finding, which was ten cores burning
after the client had stopped listening, and it does not depend on the client
implementing anything.

For the same reason nothing here is built on `notifications/progress`. It was
checked before being designed against: the client this ships to does not
populate `_meta.progressToken` on `tools/call` and does not display server
progress. A progress channel nobody renders is a fake progress bar with extra
steps; DEC-032's up-front estimate is what replaces it.

### DEC-025's restart, and the bug that shape nearly shipped

**The exec has to happen on the thread that reads.** The obvious shape — a
worker notices it answered the last outstanding request and execs — was built
first, and DEC-025's own regression test caught it hanging. The reason is worth
recording because nothing about it is obvious:

Once reading is a separate thread, that thread is *always* reading. So the
moment a worker writes the last response, the reader may already have taken the
client's next line off the pipe and into a userspace buffer, and not yet
counted it. Exec at that instant and the line is gone — and a lost request is a
hang where a failed one is an error. That is not a rare interleaving: a client
sending its next request the moment it sees the previous reply is the commonest
pattern there is, and the test hit it on the first run.

So the reader owns the exec, at the one place it holds nothing: after
dispatching what it has read and before reading again. When it sees a newer
binary it stops reading, waits on a condition variable for the in-flight map to
empty, and execs. Anything the client sends during that drain stays in the pipe
and is read by the new build.

The known edge is **unchanged, not shrunk** — an earlier draft of this decision
claimed it shrank, and that was wrong. A client that pipelines a batch can
still have siblings sitting in the reader's buffer when the drain begins, and
those are lost exactly as they were before. DEC-025's proxy parent remains the
only shape that removes it.

### What this makes newly possible, and is therefore newly risky

Two tool calls can now touch the store at once, where before they could not.
The store has been WAL with `busy_timeout=5000` since milestone 1, so a second
writer waits rather than fails, and the same overlap has always been possible
between the server and a CLI run in another terminal. What changes is the
likelihood, and the exposure is a long write — indexing a monorepo for the
first time — running past five seconds while another request wants to write.
Recorded rather than pre-solved: raising the timeout without a case that hits
it would be tuning against imagination.

## DEC-032 — A cold corpus states its bill instead of quietly running it

The field report's fifth finding: an unscoped call on ~2M units ran 20+ minutes
with no progress, no partial results and no way to estimate what was left.
Measured (PLAN.md), that call is not slow — it is a corpus-sized inference run,
about two hours and about 9 GB, standing between the caller and their first
answer. Nothing about it gets better with a spinner.

**As built: `embed::afford` refuses before any of it is paid for**, naming the
units in scope, how many have nothing embedded yet, the projected time, the
rate and thread count behind that projection, and the budget it broke. One
gate, at the one place that knows the bill — `vectors_for`, which both `search`
and `similar` reach and `dupes` never does.

### Why refuse rather than warn

A warning arrives *with the answer*, which is two hours too late; and a tool
call has no stderr to warn on before it. The three things a caller can do — ask
a smaller question, wait deliberately, or come back once something else has
warmed the corpus — all need to be offered *before* the wait, and a refusal is
the only shape that does that. It is also the house register: an ambiguous unit
name is refused rather than resolved (DEC-010), and a malformed contribution is
rejected rather than repaired (DEC-016).

**And the refusal is not a smaller tool.** Scoping the same query to a
directory at a time embeds the same corpus in the same total time, cumulatively
and for good, with an answer at every step — a strictly better shape than one
silent block. That is the sentence the message ends on, and it is only true
because `scope` now reaches every query surface (DEC-030).

### The estimate is measured, and says how well

Two rates, from the two-size scale run in PLAN.md, quoted per *thread* so the
estimate follows the machine's core count: **37 units/second/thread for `onnx`,
about 1.06 million for `hash`.** Two significant figures, which is what two
points over a 1.9× range support. What it cannot see is machine speed, so the
projection is worth a factor of two or so — ample for the judgment it makes,
which is telling three minutes from two hours, and the message says "about"
rather than pretending otherwise. An unknown embedder gets the slow rate:
over-estimating costs a refusal somebody can override, under-estimating costs
the wait this exists to prevent.

The gap between the two rates is the reason this is a *time* budget rather than
a unit count. A count that refused a 2M-unit `onnx` corpus would refuse a
`hash` corpus that embeds in a quarter of a second.

### Five minutes, and both ends of it are evidence

A cold rails checkout — 54k units, the corpus this tool was designed against
and the one its own docs call a one-time cost — is about three minutes, so the
budget has to sit above it or the documented normal becomes a refusal. The 2M
monorepo is about two hours and was abandoned at twenty minutes. Five minutes
is the round number in that gap. `CONTOUR_EMBED_BUDGET` overrides it in
seconds, `0` removes it — the same escape `CONTOUR_SEMANTIC_FLOOR` gives the
other inherited constant, and for the same reason: a threshold nobody can
switch off is an unfalsifiable one.

### What was considered and dropped

- **`notifications/progress`.** Checked before being designed against: the
  client this ships to does not put a `progressToken` on `tools/call` and does
  not render server progress. Building on it would be a progress bar nobody
  sees (DEC-031).
- **Partial results.** Worth having only where the algorithm yields them
  naturally, and this one does not: which units got embedded first is an
  artefact of a hash-ordered walk, so a "partial" answer would be a search of
  an arbitrary slice reported as a search. A scoped answer is the honest
  partial, and the caller chooses the slice.
- **Sampling the rate live** — embed a few hundred texts, time them, project.
  Self-calibrating, and it would have been wrong: each rayon worker loads its
  own ONNX session, ~3.7 s of fixed cost that a small sample measures as
  per-text and would inflate the estimate several-fold. A measured constant
  with its method written down beats a measurement taken badly.

## DEC-033 — A scope is a set of text keys, and the vector read honours it

The scale run in PLAN.md ruled out two of the three suspects and named the
third: cold cost is embedding. What it also found, in a sentence, is the floor
underneath every *warm* answer — **`Store::vectors` read the whole vector table
regardless of scope.** At 132k units that is about a second and most of the
run's memory; extrapolated to 2M it is the entire cost of every call, including
a call about one directory. Once DEC-034 makes a monorepo warm, this is what is
left.

**As built: `Store::vectors(config_key, wanted)` takes the set of text keys the
caller needs**, in chunks of 900 because SQLite binds at most 999 parameters
per statement.

### Why the caller passes keys rather than a path

The obvious shape is `vectors(config_key, scope)` with the filter in SQL, and
it cannot be written honestly. **A vector is not keyed by a unit, a file or a
path — it is keyed by the text it came from** (DEC-003), and the same text is
one row however many units share it. There is no join from `vector` to `file`
to filter on, and adding one would put a path back under layer 1, which is the
restraint that makes N worktrees of one repo cost one index.

So the scope is resolved where a scope is already known — the caller walks the
units in scope, asks `embed::text_of` what each embeds as, and hands over the
keys. That reorders `vectors_for`: it now decides every unit's text *before*
reading any vector, where it used to load the table first and look each text up
in it. The reorder is the change; everything downstream is untouched.

### What it does not fix, said plainly

`Store::units` still reads every unit in the checkout and filters by path in
Rust, so a scoped query still pays a whole-checkout cost there. That is the
next floor, and it is deliberately not attacked in the same change: pushing the
filter into SQL means writing `paths::under`'s rule a second time, in a
different language, and two spellings of "is this path in scope" that agree
today is the exact shape of a bug that arrives later.

### The memory bound in DEC-031 was written against the old behaviour

"An unscoped query holds a vector for every unit in the checkout, measured at
757 MB peak for 132k units, so four of them on a monorepo is gigabytes" — that
is still true of an *unscoped* query and no longer true of a scoped one, which
is what a session on a monorepo actually issues. Four workers stands; the
arithmetic behind it now applies to the worst case rather than to every case.


## DEC-034 — `contour embed` is where the embedding bill is paid on purpose

DEC-032 taught `search` and `similar` to refuse a cold scope that would cost
more than five minutes, and the refusal's advice was "narrow the scope". That
is true and it is not enough: on a large monorepo the only way to reach a warm
index was to name every directory in it by hand, one query at a time. The bill
itself is entirely reasonable — vectors are keyed by normalized content and
shared across every checkout on the machine (DEC-003), so about two hours
overnight buys a 2M-unit corpus once, for good. What is unreasonable is paying
it *inside a tool call*.

**As built: `contour embed [SCOPE] [--budget SECONDS]`.** It embeds every text
in scope that has no vector, commits every batch as it finishes, reports
progress on stderr from its own measured rate, and prints a `--json` /
`--ndjson` summary. `src/embed/fill.rs`.

### Why a verb, and why this verb

DEC-015 settles half of it: this fills the index, so it is a subcommand rather
than a flag. The other half is which subcommand, and the two candidates were
`contour index --embed` and a verb of its own.

`index` is the wrong host, for a reason DEC-030 already recorded: **`index`
deliberately takes no scope**, because it writes the checkout's whole file map
and a partial one is a corrupt index rather than a cheaper one. Embedding is
exactly the layer that has to be scopeable — that is the entire cost control —
so hanging it off `index` would mean one command whose two halves disagree
about what a scope is.

What it *is* is the sibling of `summarize`: one verb per expensive layer, each
taking a scope, each budgeted, each resumable, each reporting what it bought.
A reader who has met one can predict the other, which is the property that
makes a second one cheap and a third one predictable.

### The budget is seconds, and `summarize`'s is a count

The one place the two verbs deliberately differ. `summarize --budget N` bounds
*answers* because an answer is what costs money. Embedding costs time, and the
two embedders differ by four orders of magnitude in rate (DEC-032): a count
that bounded a 2M-unit `onnx` fill would bound a `hash` fill that finishes in a
quarter of a second. Seconds is the unit the thing is actually spent in, and it
is the unit `CONTOUR_EMBED_BUDGET` already speaks.

`--budget 0` is refused rather than read as "no budget" — that is what leaving
the flag off means, and one word must not mean two things two flags apart.

### It never refuses, because it is the consent

`afford`'s projection is still computed and still printed — the caller sees the
same estimate the query refused on — but nothing here can decline. Asking for
this command is what the refusal was asking for. What replaces the refusal is
the ability to change your mind: progress every few seconds, Ctrl-C at any
moment, and everything embedded so far still there.

### Resumable was free, and that is the design working

There is no checkpoint, no cursor and no "resume" flag, because a vector is
keyed by the text it came from (DEC-003) and the fill's first act is to ask the
store which of the texts in scope already have one. A second run over a warm
scope is one query and a report; a run over a half-warm scope continues.
**The interrupted-run bookkeeping is a special case that cannot arise**, which
is the cheapest kind to handle.

The one thing that had to be built for it is the commit granularity: batches of
500, under two seconds of work at the measured ONNX rate, so an interrupt loses
at most that. A fill that wrote once at the end would have lost the whole run,
and that is what the test pins.

### Not an MCP tool, and the reason is the problem statement

A two-hour tool call is the thing this exists to prevent, so exposing it as one
would close the circle back on DEC-032. Three specifics behind that:

- **Progress has nowhere to go.** A CLI has a stderr and a terminal; a tool call
  has neither, and DEC-031 already checked that this client does not render
  `notifications/progress`. The fill would be as silent as the run DEC-032
  refuses.
- **A budgeted tool call is the refusal with extra steps.** `embed --budget 120`
  as a tool would return "stopped, 1.9M texts left" — which is what the refusal
  already says, without spending two minutes to say it.
- **The session is the wrong place to spend it.** This is a per-machine,
  one-time cost that outlives the session; a person running it in another
  terminal, or overnight, is the shape that fits.

So the skill tells a session to *suggest* the command rather than to run it, and
the refusal message names it. Reconsider if a client ever renders progress and
survives a long call — nothing about the fill would have to change, only who is
allowed to start it.

### An already-warm scope exits non-zero

The house rule is 0 = something happened, non-zero = nothing did, and a fill
with nothing to fill is the second. It reads oddly beside `contour embed &&
contour search …`, which is why the human line says "already warm — nothing to
embed" rather than printing zeroes. `summarize` has behaved this way since it
shipped, and two fills disagreeing about it would be worse than either answer.

### The thing this makes urgent, recorded rather than fixed

**Vectors live in the derived half, and the derived half is named for its
schema version.** So a `VERSION` bump — which DEC-003 treats as free, because
"everything under this version is derived from bytes this machine can read
again" — now silently costs two hours on a monorepo, because the new derived
file starts with an empty `vector` table.

DEC-016 put embeddings on that side of the line with its arithmetic stated:
"re-embedding a large repo locally is about a minute". That was true of the
repo it was written about and is not true of the corpus this command exists
for. Nothing here changes it — moving `vector` into the purchased half is a
storage decision with its own migration story, and taking it in passing while
shipping a fill command would be exactly the kind of key-adjacent change
DEC-016 warns about. But **the next `VERSION` bump should not be taken as free
without reading this**, and the honest options are two: move the table, or make
the bump say what it is about to throw away.


## DEC-035 — `--profile` names the phases, and the part nobody named

A field report on a ~2M-unit monorepo could only answer "where does a scoped
query's time go?" by attaching an OS stack sampler to a release binary. It
found about a third of the main thread blocked in single-page read syscalls —
a true and unactionable answer, because nothing in it says *which* read. The
flag is the answer from the inside, and it is the same flag rq and trekr carry,
so a session that has met one can predict this one.

**As built.** `crate::profile`, vendored from rq's module of the same name and
trimmed: a phase is a named span, spans accumulate by name, and `report`/`json`
print them with each one's share of the total. `--profile` is global, so every
command has it.

### Three choices worth stating

**It goes to stderr, not into the answer.** The house rule is that any command
that prints output honors `--json`, and the tempting reading of it here is a
`profile` key on the answer object. That would change every command's output
shape for a flag most callers never pass, and it would put a measurement of a
run inside what the run *found*. So stdout stays exactly the data — a profiled
run is safe to pipe — and `--json`/`--ndjson` make the stderr report one
compact object instead of a table.

**Phases do not nest, and what they miss is a row.** Each span times one
segment of a mostly sequential pipeline; a span inside a span would
double-count and the table would stop adding up. The consequence is that some
work belongs to no phase, and the honest thing is to say how much: the
`unaccounted` row is why the first profile of a warm scoped query was worth
reading at all — 24% of it was the ONNX session load, which nothing had named
and which is now its own phase.

**A phase is a category, not an occurrence.** Two spans with one name sum into
one row carrying `xN`. That is what turned "read units 121 ms, read units 122
ms" into `read units  243 ms  41%  x2`, which is a *finding* — the query was
reading the whole checkout's unit table twice — rather than two forgettable
rows. See DEC-036.

### Refused for `mcp`, rather than quietly ignored

The server answers many requests at once on four worker threads (DEC-031), so
its phases would overlap each other and the shares would be arithmetic about
nothing. `contour mcp --profile` is an error. `CONTOUR_PROFILE` in the
environment enables the flag for a binary something else invokes — a script, a
Makefile, an editor — which is the affordance rq's `RQ_PROFILE` gives.


## DEC-036 — The near tier takes its candidates, it does not fetch them

`--profile`'s first run on a warm scoped `similar` over a 132k-unit corpus
printed one line that was the whole finding:

```
  read units        242.7ms    41%  x2  132534 rows, whole checkout
```

`near::neighbors` read `store.units(root)` and filtered it by `scope` — the
same whole-checkout read, of exactly the rows its one and only caller was
already holding, narrowed by exactly the rule that caller had already applied.
Two whole-table reads where the query needs one.

**As built: `neighbors(store, candidates, unit, threshold)`.** The caller hands
over the units; the `root` and `scope` parameters are gone with the read.
`search::similar` has already resolved the target and narrowed to the scope, so
there is nothing left for the tier to decide about *which* units — only about
which of them are near-structural neighbours, which is its job.

### Why the answers cannot move

The candidate lists differ in exactly one unit: `similar` keeps the target even
when the scope excludes it (that is DEC-030's rule — the target is the query,
never an answer), where the old read did not. It cannot matter, because the
scoring loop already drops every body whose `norm_hash` equals the target's,
and the target's does.

### What it bought, measured

Warm scoped `similar` on the synthetic corpus (132,534 units, release, 8 cores,
quiet machine, median of five with a cold run discarded): **0.60 s → 0.46 s, a
quarter of the wall clock**, and `read units` from 243 ms to 120 ms. No memory
cost, one parameter fewer, and one fewer place that spells "is this path in
scope" — which is the same argument DEC-033 made for not writing that rule a
second time in SQL.

### The test is on the profile, not on the answer

The answer never changed, so no behavioural assertion can see this. The e2e
case reads `--profile`'s own counter and pins that the phase fired **once**.
That is the instrument from DEC-035 doing the second job it was built for:
making a structural claim checkable.


## DEC-037 — Never hand rayon nothing

The field report's third finding: *"a full CPU-sized worker pool gets spawned on
every invocation regardless of workload size, including this trivial
single-file case… parked in a condition-variable wait for the entire run."*

Reproduced, and the mechanism is one line. rayon's global pool is lazy, but it
is built by whatever touches it first — and here that was a `par_iter` over an
**empty** list. Every command opens the index, opening it refreshes it
(DEC-018), and on a checkout where nothing moved there is nothing to parse. So
every warm command built a thread per core, parked them, and tore them down.

**As built: `crate::pool::map`, which returns early for an empty slice** and is
otherwise the same `par_iter` over the same one global pool. Two call sites —
the parse loop and `embed_all`. DEC-031's worker bound is untouched: the heavy
work still goes to one shared pool, so N server workers still do not become N
pools.

### The threshold this deliberately does not have

The obvious next step is a size below which work runs inline, and the
measurement refused it. On 8 cores, building the pool costs about **0.4 ms**
(50 warm scoped queries in 6.17 s against 6.15 s, and 194 involuntary context
switches against 114), while one blob parse costs about **0.3 ms** of CPU
(17,639 blobs, 5.26 s of user time). A fixed cost that small pays for itself at
about **two** items, so any threshold worth naming would make real work serial
to save a fraction of a millisecond — a bigger regression than the fix.

The one loop with a genuinely large crossover is embedding, where each worker
loads its own ONNX session; `embed::embed_all` already carries that measurement
(529 units: 3.7 s serial against 3.9 s pooled) and its own decision, and nothing
here changes it. One rule, no number to keep calibrated.

### What this finding was *not*, said plainly

The report paired the idle pool with "`sys` ran about 2× `user`" and read the
two as one cause. On this machine they are not: a trivial warm scoped query is
0.12 s user against 0.02 s sys, and removing the pool moves the wall clock by
0.4 ms of 123 ms — about 0.3%. The parked threads were real and are gone, but
**the system time on a monorepo is the page reads of finding 1, not thread
creation** (DEC-036, DEC-038). Worth recording, because "we fixed the pool" is
otherwise an answer somebody could mistake for an explanation.

### Testable, in its own test binary

`ThreadPoolBuilder::build_global` succeeds exactly once per process, which is
the only way to assert from inside that nothing built a pool. That makes the
probe meaningless in any process where another test has touched rayon — and
cargo runs a file's tests as threads in one process — so it lives alone in
`tests/pool_lazy.rs`, with the cold index (which really does parse blobs) run
as a subprocess.


## DEC-038 — The index is not memory-mapped, and the measurement is why

A field report stack-sampled a warm scoped `similar` on a ~2M-unit monorepo and
found about a third of the main thread blocked in single-page reads. The
direction was to lean into memory-mapping. Two candidates were measured;
**neither ships**, and the reasoning is recorded because "map the index" is an
idea that comes back.

### `PRAGMA mmap_size` — rejected

One line, and it was measured at two corpus sizes, warm and with the page cache
evicted (`docs/PLAN.md` has the tables and the method). Warm it is inside the
run-to-run spread. Cold it is a wash at 132k units and **21% slower** at 256k.
Peak RSS is **45–50% higher** at every size.

The per-phase split is the part to keep: **mapping helps a sequential table
scan and hurts a random key lookup.** At 256k, cold, `read units` goes 763 →
526 ms and `read signatures` 287 → 135 ms, while `read vectors` — 323 key
lookups — goes 541 → 1,238 ms. contour's warm path is two sequential scans plus
a few hundred seeks, and the seek loss grew *with the corpus* (981 ms at 132k,
1,238 ms at 256k for the same 323 keys) where it had been predicted to stay
flat. A deeper b-tree over a larger file is the reason, and predicting
otherwise is what the measurement caught.

The pragma also carries a cost that never appeared on the clock: an I/O error
on a mapped page arrives as a signal rather than an error code. That would have
been acceptable for a derived file that rebuilds in seconds. It is not
something to accept for nothing.

### DEC-033's abandon threshold, re-asked under the map

The read-through/per-key crossover was measured before mmap, so a mapped file
had to be assumed to move it — cheaper to read through, so abandon later. It
moves the other way. With the read-through never abandoned, the cold vector
read is **2,919 ms against 981 ms** and peak RSS **785 MB against 305 MB**.
`SCAN_RATIO` stays at 3.

### The flat memory-mapped vector file — not yet earned

The structural candidate: a file beside the database holding a header, sorted
`keys[]` and `vecs[]` of dims×f32, built when `embed` completes, swapped in by
atomic rename, mmap'd read-only, binary-searched, cosine run over the slices in
place. Derived and rebuildable, stamped so a mismatch means rebuild — DEC-016's
rule, correctly applied.

It is a real design and it addresses **1%**. `--profile` on a warm scoped query
over 132k units puts the vector read at **6.1 ms of 457 ms**, because DEC-033
already made it scope-sized. Its whole opportunity is the cold read — 541 ms of
2.13 s at 256k, paid once per corpus per boot — bought with a new on-disk
artifact, a version stamp, a staleness rule, a rebuild path, an alignment
constraint and its own tests. That is a cathedral over a puddle.

**What would change this ruling**, stated so it is falsifiable rather than
permanent: the vector read becoming a large share of a *warm* query. It is 1%.

### What was actually costing the time, and what is next

The reads that matter are the two nobody had named: `Store::units` and
`Store::signatures`, both **unscoped whole-table** reads, together 45% of a
warm scoped query after DEC-036. `read summaries` is a third, invisible in the
measurement here only because the synthetic corpus has no summaries — the
reporting corpus does. Scoping those the way DEC-033 scoped the vectors is the
next floor, and it is the same trade DEC-033 declined to make blind: it means
writing `paths::under`'s rule a second time in SQL.


## DEC-039 — A guard is summarized by what it refuses, not by how it checks

The field report's ranking complaint, twice: a question about *preventing an
action once a resource is finalized* put the one guard-clause method that
implements exactly that check at rank 17, behind nine methods that merely call
it. PLAN.md reproduced it on the fixture and separated two mechanisms. This is
the second one, and it is not the scorer's.

**The evidence.** Rewriting only the guard's summary, changing nothing else:

| the guard's summary | rank | cosine |
| ------------------- | ---: | -----: |
| "Signals an error unless the shipment is still open, so that nothing may change once it has been finalized." | 4th | 0.28 |
| "Refuses any change to a shipment that has already been finalized." | **1st** | **0.39** |

Both are accurate. The first says how the check is spelled, the second what the
caller is prevented from doing — and a guard clause is precisely the shape where
those diverge, because its implementation is a negation of a negation while its
contract is a plain refusal. The methods that call it state the contract in
passing, so they win against a guard that states its mechanism.

**As built: one rule, in both places a summary is written from.** The API
prompt (`summary::prompt::SYSTEM`) and the skill's "rules that make a summary
worth storing", worded the same and carrying the same worked example.

### Only one of the two versions moves, and that is the existing rule

`PROMPT_VERSION` goes v2 → v3, because its frozen test says any change to that
file does — and it is free for the same reason v2 was: no API fill has ever
run, so nothing is keyed under v2.

`CONTRIBUTED_PROMPT_VERSION` does **not** move, and that is not an oversight.
Its own pinning test already settled which side of the line prose is on: the
version moves when the *schema* moves — a field, a vocabulary word, something
newly refused — and never when the skill's prose is clarified, because every
wording fix would otherwise strand summaries a session already paid for. A
guidance line is prose.

### What this therefore does not fix, said plainly

**It repairs nothing already summarized.** A corpus whose guards were described
by their mechanism keeps those summaries, `pending` will not offer them again,
and the reporting corpus does not improve on upgrade — only on re-summarizing.
Forcing that by bumping the contributed version was considered and refused:
stranding every paid summary on every machine, to re-buy one shape, is a worse
trade than the shape is worth.

### And it is the one change here that could not be scored

Confirming a prompt change needs a real summarizer over a real corpus, which is
an API bill this session did not have. The 4th → 1st above is measured on
hand-authored fixture text, which `tests/eval/README.md` already flags as an
authored input rather than as evidence about ranking. It ships as *guidance*
for that reason, where the scoring half of the same case (PLAN.md's mechanism
A) shipped as a scored change with eleven top-1s and no losses behind it.
