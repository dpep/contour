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
| `mastodon/` | a mastodon checkout | real summaries, so an API key |
| `rust/<repo>/` | the sibling repos (trekr, rwr, rq, gqls, ae, launder, navi) | nothing |
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

### Where a label may come from

**A label is never sourced from the feature it evaluates.** A set drawn from a
tier's own output can only ever confirm that tier, and it will do so
convincingly.

This is not hypothetical. rails' `near` labels were found by reading the near
tier's report, and scored 1.00 recall. discourse's were found by sweeping the
threshold down to 0.55 and reading what turned up — a method that can see what
the tier *misses* — and scored 0.50 on the same code. The gap is the
methodology, not the corpus. **rails' near labels need a discourse-style
re-audit**; until then, read that 1.00 as an artefact.

The general form: find candidates with a method the feature does not share.
Sweep below the threshold, read the source, use a different tool, or start from
the behaviour rather than from the report.

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

`abstain.tsv` — proposed format, no harness reads it yet — the complement of
`canonical.tsv`: pairs where declining to crown either side is the CORRECT
answer, so that abstention has ground truth of its own instead of being
counted merely apart from wrong (DEC-019 says declining on disagreement is
the design; this makes that scoreable). One row per pair,
`a<TAB>b<TAB>why[<TAB>provisional]`, `why` from a small vocabulary:

- `siblings` — both copies born in one commit, so no copy direction exists
  for any signal to find (rails' two `write_query?` adapter copies, f39d72d526;
  discourse's two locale updaters, cb739f7f2df).
- `front_doors` — each copy is the live entry point of its own stack, and
  the true home is an extraction that has not happened yet (mastodon's rack
  vs sidekiq socket cleanup).

A pair may appear in `pairs.tsv` as `duplicate` and here as expected-abstain:
"consolidate these" and "neither is the original" are different questions,
and the sibling rows are exactly where both are true. When the harness learns
to read this file, naming either side of a row scores as wrong.

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

**The harness reads it**, running `contour similar` exactly as a caller would
— the default limit, the corpus's own path policy — because what is being
scored is what somebody is *shown*. A `must` row is correct only when the
neighbour is found by the tier it names: the tier is a claim (DEC-019), so
finding a labeled-semantic pair at the near tier is a wrong answer, not a
better one. A `must_not` naming a tier forbids that tier and every stronger
one.

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

## What mastodon adds: the transfer verdict, third corpus

discourse asked whether rails-calibrated thresholds transfer to an app; the
mastodon set asks whether the *discourse-era fixes* transfer to a second app.
Measured on a 2026-08 checkout (structural tiers; the search half awaits
summaries):

- **Path classes carry over.** 23 all-migration groups withheld, test
  helpers ranked as their own population, `mixed` groups still reported —
  discourse's migration FP class does not recur. Its residue does: a
  migration that embeds its own copy of an app model's method survives as a
  `mixed` group (`MigrateAccountConversations::MigrationAccountConversation`),
  reported and still not consolidatable — labeled `distinct`.
- **Exact precision 0.67 (12/18, recall 1.00), and the new FP class is RAILS
  CONVENTION DISPATCH**: byte-identical mailer actions (7-strong UserMailer
  group, plus AdminMailer's trio) whose behaviour differs only through the
  method's own name — template lookup and i18n subject key both derive from
  it. The `super` arithmetic (DEC-017) through a convention normalization
  cannot see, and no path class can fence: they are app code. A fix would
  have to know what a mailer is.
- **Near recall 0.44 at 0.70 (4/9, precision 1.00)** — the discourse finding
  reproduced on an independent corpus and an independently sourced label set
  (0.55 sweep + reading): genuine one-edit copies land at 0.57–0.67
  routinely, including a drifted-bugfix pair where the remote-edit service
  gained two guards the local-edit copy lacks.
- **Canonicality is unmeasurable here, and that is itself the finding**: the
  checkout's git history is one squashed commit, so `git_age` — the signal
  that carried 4/5 discourse edges — is structurally silent, and no in-repo
  delegation exists to substitute. `canonical.tsv` documents why it is
  empty; canonicality ground truth is a property of a corpus's provenance.
- **A Ruby same-id twin exists** (two files defining `Paperclip::LazyThumbnail`
  — ImageMagick original, libvips port; only the port is still required, so
  the original is dead code). The same-id pair convention the Rust sets
  introduced is now exercised by a Ruby set too.

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

Two limits, noted rather than worked around: the harness runs one checkout,
so the cross-repo copy (ae's `HashEmbedder::embed` is a paste of gqls's,
identical modulo locals) is real but unlabelable; and `ae` itself has no
in-repo pair worth labeling. Measured baseline on 2026-08 checkouts: every
labeled collision reported, no false collisions at any size, and one
deliberate false negative (rwr's 3-line `corpus_dir` sits below
`--min-lines 4` — the floor's cost, on the record).

One labeled family has since been RETIRED WITH ITS DUPLICATION: rq
consolidated the epoch-seconds helpers into `core/clock.rs` (7df14e2, "One
clock, not four", 2026-08-27), which is the outcome the report exists to
cause. The rq pair and similar labels were relabeled to the new corpus; the
qualification-twin contract case went with them (rwr's renamed-local rows
still cover the normalization boundary).

### Rust search: the first query labels

The Rust sets originally measured only the duplicate tier; a field trial
then found Rust *search* weak — sentence-named test functions dominating —
so `queries.tsv` across the seven repos now carries the first Rust search
ground truth (~20 queries), in the same bands as the Ruby sets plus one
convention of its own:

- **TEST-TRAP rows** (marked in comments): the answer is production code,
  but the query's wording deliberately matches the repo's sentence-named
  tests (`a_bare_call_in_a_class_body_dispatches_on_the_class`, ...). These
  are the regression net for `search::NON_APP_DISCOUNT` — and today they
  fail, because Rust's inline `#[cfg(test)] mod tests` sit in app files and
  classify as `app` (DEC-022's standing example), so the discount never
  touches the very units the field trial tripped on. On rq, the ranking for
  a trap query is a page of `tests::*` units with the production answer
  below the fold. Fixing that requires unit-level test classification — and
  the `tests::` owner prefix already visible in those units' ids is the
  signal to build it from.

### Cross-language queries: a proposed extension

Some ideas exist twice across corpora, once per language: discourse's
`ScreenedEmail.levenshtein` and ae's `levenshtein`, rails' camelize and
gqls's `to_pascal`. The ae and discourse sets carry the SAME query verbatim
("how many edits turn one string into another"), each labeled with its own
corpus's answer — the harness runs one checkout, so that is as far as the
current format reaches. Proposed, for when it matters: an optional
`corpus:` prefix on the expectation column
(`ruby/discourse:ScreenedEmail.levenshtein<TAB>rust/ae:levenshtein`), scored
only by a future multi-corpus runner and ignored by the one-checkout
harness. Not built until a product feature needs cross-corpus answers.

## The near sweep, and what it can and cannot settle

`contour eval` prints a **near-structural calibration** table: both candidate
measures (`shapes`, `nodes`) across thresholds, with precision, recall, and the
short band counted separately. That table is where `near::NEAR_THRESHOLD` comes
from, and it replaced the four-labels-a-side argument that produced 0.80.

**It cannot compare the two measures, and the reason is this file's own rule.**
Every positive in both label sets was surfaced by the *shape* measure — rails'
by reading its report at 0.80, discourse's by sweeping it to 0.55. A pair the
node measure would find and the shape measure scores below the sweep floor
cannot be in the labels at all, so the comparison can only ever confirm shapes.
The label-sourcing rule applies to a **measure** exactly as it applies to a
threshold.

What would settle it: sweep the *node* measure down on discourse, read what
turns up, and label it. Those labels plus these would let either measure be
scored against candidates the other found. Until then the shape measure decides
and the node counts price the consolidation.

## The rails re-audit, and what it did to the near numbers

rails' near labels were sourced from the tier's own report at 0.80, so they
could only ever confirm it — 1.00 precision, 1.00 recall, and a number that
meant nothing. M11c re-drew them the only way that answers "how often is this
tier right": **sweep to 0.55, take a random sample of what it reports, judge
every pair in the sample.** Seed 11, 20 pairs from what ships at 0.70 and 15
from 0.55-0.70, each read against the rule.

**The finding: 2 of the 20 shipped pairs are real consolidations. Precision
0.10.** Eighteen are sibling test methods that differ by exactly the thing they
test — `test_becomes` against `test_becomes_after_reload_schema_from_cache`,
`destroy` against `delete`, one option set against another. Nineteen of the
twenty are test code. The two real ones are duplicated test *helpers*
(`open_connection` in two ActionCable test classes; two latency stubs in
ActiveSupport's cache tests), which somebody could actually consolidate.

Lowering the threshold buys almost nothing: **1 real pair in 15** sampled from
0.55-0.70.

**The printed rails precision is a blend and reads high.** The harness scores
every labeled pair together, and rails' file now holds two populations: the ten
original labels, drawn from the tier's own output and therefore true by
construction, and this random sample. 0.52 as printed mixes them. **0.10 is the
unbiased estimate**; the printed number is an upper bound on it.

**This is evidence the M11b threshold ruling did not have.** 0.70 was ratified
because it improved precision *and* recall over 0.80 on the labels as they then
stood. On the re-audited labels, merged across both corpora, it is a trade
rather than a free win:

| threshold | precision | recall |
| --------- | --------- | ------ |
| 0.70 | 0.47 (24/51) | 0.67 (24/36) |
| 0.80 | 0.55 (18/33) | 0.50 (18/36) |

That is a decision, not a calculation, and it belongs to whoever owns the
threshold. What the re-audit settles is that it must be made on these labels
rather than the old ones.

**It still cannot compare the two measures.** The sample was drawn from what the
*shape* measure reports, so it estimates that measure's precision honestly and
says nothing about pairs only the node measure would find.

## The short-body band: the near tier's limitation, measured

Jaccard is harsher on short bodies — one edited line moves a third of an
8-line body's tokens — and until now that was a paragraph in the docs rather
than a number. `pairs_short.tsv` in the rails and discourse sets holds 4–8
line pairs, same grammar as `pairs.tsv`, kept apart so the band's numbers
never blur the headline ones. **The harness reads it** as its own population,
and the near sweep gives it its own column — including how many short *distinct*
pairs a threshold wrongly reports, because recall bought with precision is not
an improvement.

Measured at the calibrated settings (threshold 0.80, `--min-lines 4`):

- **11 of 13 genuine short near-duplicates are missed** (rails 0/5,
  discourse 2/8). The copies score 0.56–0.76 — a one-line edit in a 5–9 line
  body lands far below a threshold that was calibrated on longer bodies.
- **The two the tier does catch are the rename-only pairs** (jaccard 1.0): a
  changed parameter or keyword name leaves the token multiset intact, so
  shortness costs nothing. The variable is edits-per-token, not length.
- **Both labeled false positives sit at exactly 0.80**: opposite controller
  actions (pin/unpin, banner/pin) whose shared scaffold is most of a short
  body. Lowering the threshold to recover the misses would let these in —
  on this evidence the band is not fixable by moving the constant, which is
  the finding.

**Where M11 left it: 6 of 13, at threshold 0.70.** Better, not fixed — and not
fixed by the change of measure it was supposed to be fixed by. See the sweep
section above for why that comparison is not available on these labels.

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
