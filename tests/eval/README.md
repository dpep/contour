# The eval set

The labeled data that settles thresholds (DEC-011). Every threshold in contour
is otherwise inherited from another corpus or guessed: the relevance floor came
from gqls's measurements on GraphQL records, and `--min-lines` came from a
distribution on rails. This is how they stop being that.

```sh
cd <checkout> && contour eval <set-dir>
```

## The labels are a draft

**Everything here was drafted by Claude and has not been reviewed by the
owner.** Each label is a judgement about what a person would mean by a query,
or about whether two methods really do the same thing — and a wrong label makes
the eval lie in the flattering direction, because it is graded against itself.

Veto or amend any line. The harness reads these files directly, so an edit is
the whole change. Labels the drafter was unsure of carry a `provisional` flag
in the last column and are counted separately in the report.

## Sets

| set | corpus | needs |
| --- | ------ | ----- |
| `fixture/` | `fixture/corpus/` — 21 methods, in-repo | nothing; runs in CI |
| `rails/` | a rails checkout | real summaries, so an API key |
| `discourse/` | a discourse checkout | real summaries, so an API key |
| `rust/<repo>/` | the sibling repos (trekr, rwr, rq, gqls, launder, navi) | nothing |
| `berater/` | a berater checkout — `similar.tsv` only | nothing |

`fixture/` exists to prove the machinery, not to calibrate anything: two dozen
units across four domains is far too small and too thematically dense for the
distractor distribution to mean anything. Read its numbers as "the harness
computes what it claims to", never as evidence about a threshold.

It does now separate the two vector tiers, which it originally could not. Four
queries name behaviour that appears in a body and nowhere in a name — a retry
with backoff, a memoized lookup, an idempotent write — and the identifier tier
is structurally unable to answer them:

| ranking | top1 | top5 | found |
| ------- | ---- | ---- | ----- |
| contour (summaries where they exist) | 0.27 | 0.55 | 22/22 |
| contour:identifier (names only) | 0.23 | 0.45 | 21/22 |

That is what summaries buy, on a corpus small enough that the honest headline
is still "too small to conclude from".

## Format

`queries.tsv` — what a person might ask, and the one method that answers it:

```text
customer owes us money	Invoice#unpaid_for
bills a client has not settled	Invoice#unpaid_for	provisional
```

Queries deliberately avoid naming the method. A query containing its own answer
measures string matching, which is what `baseline:name` is there to isolate.

`pairs.tsv` — pairs that should and should not be reported as duplicates:

```text
Warehouse#available	Depot#available	duplicate
Invoice#unpaid_for	Receipt#outstanding	distinct
```

The `distinct` rows are the sharp half. Each is a *near* miss that a looser
normalizer would collide, so if one ever shows up as a duplicate, normalization
has been over-relaxed.

### The labeling rule

**A pair is a duplicate iff consolidating it would reduce net complexity.**
Ratified by the owner, and the single question every `duplicate` / `near` /
`distinct` verdict here answers:

> we're aiming to uncover duplicates as behavior-preserving consolidation
> candidates. but if elaborate alterations or metaprogramming are required to
> consolidate, they don't seem like duplicates (at least not worth
> deduplicating, because we're trying to decrease complexity not increase). a
> guide: does consolidating two methods reduce/contain complexity? if so,
> decent candidates. and some near-duplicates can be consolidated with a few
> modifications... which is great to surface.

This is a **product definition**, not a labelling convenience. `dupes` exists to
offer a consolidation; a pair whose consolidation does not exist is not a
finding, it is a wasted trip. What the rule settles here:

- The four `super` pairs (DEC-017) and the `compatible_table_definition`
  constant-scope pair are **distinct**. Each copy has to stay a separate link in
  a separate dispatch chain, so consolidating needs metaprogramming or a
  behaviour change — complexity up.
- `write_query?` and `to_fs` are **duplicates** despite the same
  context-dependent-constant shape, because a small parameterization
  consolidates them — complexity down. The shape does not decide; the
  arithmetic does.
- `Macaddr#changed?` / `#changed_in_place?` and the two Arel comparison
  visitors are **near**: one shared helper, two thin callers. That a few
  modifications are needed is the point rather than a caveat.

It also fixes what the near tier is *for*. "Same except a few lines" pairs are
often the **best** consolidation candidates — a small edit for a real
reduction — so the report's implicit ranking is expected complexity reduction
per unit of effort, not similarity.

`canonical.tsv` — groups of similar methods where one is unambiguously the
implementation the others shadow, with the evidence named (Phase 3's
canonicality signals need ground truth too). One row per (canonical,
alternate) edge:

```text
Invoice#unpaid_for	LegacyInvoice#unpaid_for	older,delegates
```

The third column is comma-joined from a small vocabulary — `older` (canonical
predates the alternate in git), `delegates` (the alternate calls the
canonical), `mirrors` (a declared copy, e.g. a test stub), `ex_subclass` (the
alternate's class once inherited the method's home). Every verdict is verified
by reading both bodies and `git log`, never assumed.

The evidence column is **validated but not scored against** — an unknown token
fails the run, and a signal's job is to be right rather than to be right for
the reason a human wrote down. The file is optional: a set without one simply
reports no canonicality section.

The pairs here are *edges*, not groups, and deliberately not all duplicates: a
shim that `delegates` to what it shadows has a different body, so no clone
group holds it, and it is still a pair worth ranking. The harness ranks each
labeled pair directly for that reason.

`similar.tsv` — ground truth for `contour similar`, the flagship agent tool,
which until now had none. Proposed the way `canonical.tsv` was: file first,
harness later. One row per (probe, assertion):

```text
DateTime#advance	must	Time#advance	near_structural
TopicsController#re_pin	must_not	TopicsController#make_banner	near_structural
Berater::ConcurrencyLimiter#acquire_lock	must_not	Berater::Lock#capacity
```

- `must` — the neighbour must appear within the default limit, and the tier
  column names the tier expected to find it (`structural | near_structural |
  semantic`). The tier is asserted because it is a claim (DEC-019: structural
  and near mean "the same implementation twice"), not a detail.
- `must_not` — with a tier: the neighbour must not appear at that tier **or
  stronger** (a related method may be a semantic neighbour while a near claim
  would be wrong — the DEC-017 pairs are exactly this). Without a tier: must
  not appear at all.

Every case was verified by running `contour similar` from the corpus checkout
and reading the bodies. Cases the tool currently fails are labeled with what
SHOULD happen and marked `CURRENTLY FAILING` in a comment — those are the
point of the file: the near tier presenting opposite controller actions as
near-copies at exactly 0.80, and identifier-only noise (a `to_s`, an
attr_reader) outranking real siblings in a method family. The berater set
exists for this file: one `acquire_lock` contract, sibling implementations
across limiter classes — the shape `similar` was built to answer.

The verified-against state is identifier-only vectors (`coverage none`); a
summarized corpus should only improve the semantic rows, but the expected
tiers were chosen to be true under either.

## The Rust sets: measuring the token_hash tier

Rust's normalization is a degraded tier on purpose (DEC-012): a
comment-stripped token stream that catches copy-paste and reformatting, where
a renamed local, a changed literal, or a `std::time::`-qualified path moves
the hash. It shipped unmeasured; `rust/<repo>/` is its ground truth. Each
subdirectory runs against one sibling checkout:

```sh
cd ~/code/lib/rust/trekr && contour eval <this repo>/tests/eval/rust/trekr
```

Two conventions specific to these sets:

- **Distinct labels here come in two flavours, and the comments say which.**
  Most are product judgments as everywhere else (one differing token IS the
  behaviour — a builder setter, a per-plugin constant). A few are the tier's
  *contract*: a genuine duplicate that differs only by a renamed local
  (rwr's `line_start`, gqls's `pascal_case`/`to_pascal`) is labeled
  `distinct` because the tier not catching it is correct-by-design — those
  labels flip if normalization ever reaches Prism-grade parity.
- **A pair may name the same id twice.** Rust free functions have no owner
  (DEC-012), so a helper copied between two files is two units with one
  name, and `git<TAB>git<TAB>duplicate` asserts "the two same-named units
  collide". Sound while exactly two units carry the name; a third would
  make the label ambiguous, which is the known cost of DEC-012's owner rule.

`queries.tsv` is deliberately empty — the duplicate tier is what shipped
unmeasured. Two limits, noted rather than worked around: the harness runs one
checkout, so the cross-repo copy (ae's `HashEmbedder::embed` is a paste of
gqls's, identical modulo locals) is real but unlabelable; and `ae` itself has
no in-repo pair worth labeling. Measured baseline on 2026-08 checkouts:
every labeled collision reported, no false collisions at any size, and one
deliberate false negative (rwr's 3-line `corpus_dir` sits below
`--min-lines 4` — the floor's cost, on the record).

## What discourse adds: do rails-calibrated thresholds transfer?

The rails thresholds were calibrated on one corpus, and a library's twins
(`String#first`/`#last`) are not an app's twins (copied import scripts,
plugin-to-plugin copies, migrations). The discourse set exists to ask whether
the numbers hold. Measured on a 2026-08 checkout, structural tiers only:

- **Exact-tier precision drops from 0.93 to 0.73**, and every new false
  positive is a schema migration: byte-identical `up`/`down` bodies that are
  frozen history, never consolidation candidates (DEC-020). `--min-lines`
  cannot help — the largest is 16 lines. This is a *class* of collision rails
  had no label for; a fix would have to know what a migration file is. Recall
  stays 1.00.
- **Near-tier recall drops from 1.00 to 0.50 at jaccard 0.80.** The rails
  near labels were drawn from pairs the tier already reported; the discourse
  ones were sourced by sweeping the threshold to 0.55 and reading, so the
  number measures the threshold against the copy-paste population instead of
  against itself. Genuine one-edit copies score 0.56–0.73 routinely (a 9-line
  copy with one guard changed lands at 0.58). Precision holds (one false
  positive: a migration's own `up` against its `down` at 0.83).
- **Canonicality improves**: 4/5 edges correct (rails: 2/5), because app
  copies have cleaner provenance — `git_age` alone went 5/0. The one
  abstention is a plugin merged from an external repo, whose pre-merge
  history git cannot see.

## What the report says

- **search** — rank of the expected method, as top-1 / top-5 / found-at-all,
  for contour and for two baselines. `baseline:name` is token overlap against
  the method's name (what fuzzy matching alone gives you); `baseline:source`
  is token overlap against the method's body (what `rg` gives you, and
  deliberately generous — it reads code contour's semantic half never sees).
- **duplicates** — precision and recall over the labeled pairs, plus the
  smallest true duplicate and largest false collision, which is where
  `--min-lines` belongs.
- **canonicality** — for each labeled edge, whether the signals named the
  labelled implementation, named the other one, or declined to choose. An
  **abstention is counted apart from a wrong answer**: declining when the
  signals disagree is the design (DEC-019), and folding the two would make
  guessing look like an improvement. Each signal is also scored on its own,
  which is what says whether it earns its cost.
- **calibration** — the cosine of each labeled answer against the best wrong
  answer for the same query, swept across candidate floors. This is gqls's
  floor experiment rerun on method summaries.

The eval runs search with **no floor**, deliberately. Calibrating a threshold
against results that threshold already filtered can only ever confirm it.

## Running the rails set

```sh
cd ~/code/lib/ruby/rails
contour index
contour eval ~/code/lib/rust/contour/tests/eval/rails
```

The **duplicate half runs today** — it needs only the structural hash. The
**search half needs real summaries**, so it needs an API key:

```sh
contour summarize --budget 500     # or however much of the corpus you want
contour eval ~/code/lib/rust/contour/tests/eval/rails
```

Search numbers against a corpus with `coverage none` are not a result. The
report says which state it ran in on every line of output, so a run without
summaries cannot be mistaken for one with them.
