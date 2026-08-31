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

### The other way to define this out of existence

Worth writing down because it is a smaller idea than either restart mechanism:
**version the derived database in its filename.** DEC-016 already splits the
schema into a derived half that may be dropped and a purchased half that may
not. If the derived half lived in `contour-v10.db` beside the purchased
`contour.db`, a v9 binary and a v10 binary would each build their own and never
collide — no skew, no restart, and the "two contours take turns wiping each
other's index" hazard that `Store::init` guards against would stop existing too.

Not built, and not because it is wrong: it is an on-disk restructure (an
`ATTACH`, every derived query qualified, a one-time split of existing
databases), which is a structural change on top of a behavioural one. Left for
the owner to rule on.

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
