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
