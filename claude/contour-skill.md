---
name: contour
description: Semantic code index — find code by what it DOES, not what it is called, and check whether an implementation already exists before writing one. Use for behavioural questions ("which methods retry on failure", "where do we settle an invoice"), for "is there already something that does X" before adding code, for duplicate checks (`contour dupes --near` sees through renames and reformatting, which grep cannot), and for `contour --symbols FILE` to outline a file before reading it. Ruby and Rust. Complements rq (where is this NAME defined) and trekr (what does this Ruby call actually run); contour is the one to reach for when you know the behaviour but not the name. Contribute summaries back as you read code — see "Feeding the index".
---

# contour — find code by what it does

`rg` finds text. `rq` finds a name. `trekr` resolves a Ruby call. **contour
finds behaviour** — the method you would recognise if you saw it, whose name
you cannot guess.

```sh
contour search "customer has not paid their bill"   # behaviour, in English
contour similar 'Invoice#settle!' app                # does this already exist?
contour dupes --near                                 # copy-paste, incl. tweaked
contour dupes app/models --canonical                 # ...and which copy to keep
contour --symbols app/models/invoice.rb              # outline before reading
contour --status                                     # what the index knows
```

Every answer says how it was found and what it could not see. Read those
fields — they are the difference between an answer and a guess.

## When to reach for contour instead of the others

| the question | tool |
| ------------ | ---- |
| "where is `Invoice` defined" | `rq` |
| "what does this call actually run" / "who really calls this" | `trekr` |
| "which method sends the overdue reminder" | **contour search** |
| "do we already have something that does this?" | **contour similar** |
| "is this method copy-pasted anywhere?" | **contour dupes --near** |
| "what's in this file" | **contour --symbols** (or `rq --symbols`) |
| a literal string, a TODO, a config key | `rg` |

The rule of thumb: **if you can name it, use rq; if you can only describe it,
use contour.**

## Before you write a new method

This is contour's highest-value moment and the easiest to skip.

```sh
contour search "what you are about to build" -l 5
```

If something comes back with a plausible summary, read it before writing. The
cost of the check is one command; the cost of missing it is a fourth
implementation of the same thing. `contour similar 'Owner#method'` does the
same job once you have a candidate.

## Reading an answer honestly

`search` discloses two things that change what its answer is worth:

- **`coverage`** — `none`, `warming`, or `complete`, with the fraction. At
  `none` nothing has been summarized and the semantic half matched on what
  code is *called*, not what it *does*.
- **`semantic_via`** on each hit — `summary` (matched on behaviour) or
  `identifier` (matched on the name). An `identifier` hit is blind to anything
  that never appears in a name, which is exactly where a summary would have
  earned its cost.
- **`name`** (`lexical` in JSON) and **`cos`** (`cosine`) — the two halves'
  measurements, each 0 to 1. `name 0.10` means the unit's name accounted for a
  tenth of your question; `[both]` on such a hit is one strong signal and one
  whisper, not two votes. A high `cos` with a low `name` is the good case: it
  matched what the code *does*.
- **`via <Container> — its container's only public unit`** — a class answered
  for one of its methods. Entry points are named for the protocol they
  implement (`call`, `to_s`, `get`, `hydrate`), so the behaviour you searched
  for lives in the class's private helpers and the method you can actually call
  says nothing about it. Where a class has one public method, contour lets the
  class speak for it. **Treat it as a pointer to the right neighbourhood**: the
  class is what matched, so read it, and expect the specific helper you wanted
  to be inside.

`similar` discloses a tier per neighbour: `structural` (identical normalized
body), `near_structural` (mostly the same shape, with a measured Jaccard), or
`semantic` (a nearby summary, with the cosine). A `structural` match is a fact;
a `semantic` one is a suggestion.

`similar` also takes a **scope**, the same second-positional path `search` and
`dupes` take, and it answers with the `scope` it searched. The unit you ask
about is found wherever it lives; the scope bounds the *answers*.

### On a big repository: refused → `contour embed` → warm forever

Every unit in scope needs a vector, and one that has nothing embedded yet is
embedded on the spot — measured at ~295 units/second with the real embedder, so
a cold million-unit scope is an hour before the first answer, not a slow query.
contour refuses that rather than running it, and the refusal tells you how many
units it weighed and what it would have cost.

**Two ways out, and the first is the durable one.**

1. **Pay it once, deliberately, outside the session**: `contour embed` (or
   `contour embed <dir>` for part of the repo). It embeds only what has no
   vector, commits as it goes, prints progress with a measured rate and an ETA,
   and continues where an interrupted run stopped. `--budget SECONDS` bounds a
   sitting. Vectors are keyed by content and shared across every checkout on the
   machine, so an overnight run is paid once and never again. **Suggest this to
   the user rather than running it inside a tool call** — the whole reason for
   the refusal is that a two-hour call is not a tool call.
2. **Name a directory** for the answer you need now: `contour search "..."
   app/billing`, `contour similar 'Owner#method' lib/`, `dupes app/models`. Each
   scoped run keeps what it embeds, so working through a repository a directory
   at a time warms it cumulatively and answers at every step.

Once a scope is warm, asking it again is fast, and a scoped query pays only for
the vectors in its scope.

If `search` returns less than you expect, run `contour --status` before
concluding the code is not there.

**Answers are never stale.** Every query brings the checkout up to date before
answering — a file you just edited or deleted is accounted for, and the answer
says `refreshed` (a tool result) or `index refreshed` (on stderr) when that took
work. You do not need to run `index` after editing.

**Phrase the question the way you would ask a colleague, but know what it
costs.** Measured on the eval: against a *summarized* corpus a full sentence
beats a keyword-shaped query (14/22 top-1 against 10/22), because the filler
words are signal to the meaning half. Against an *unsummarized* one it is worse
(1/21 against 4/21), because those same words are tokens no identifier can
match. Check `coverage` first: at `none`, ask in keywords; at `warming` or
better, ask in sentences.

## Outlining a file

`contour --symbols FILE` parses the file in front of it — no index needed — and
marks anything that is not public:

```text
    4  StatusCacheHydrator#initialize(status)  [private]
    8  StatusCacheHydrator#hydrate(account_or_id, nested: …)
   29  StatusCacheHydrator#hydrate_non_reblog_payload(...)  [private]
```

The unmarked lines are the file's interface, which is usually what you came for.
Ruby's `initialize` is private however it is written, and Rust's `pub(crate)`
shows as `protected`.

## What kind of file each answer is in

Every hit, neighbour and duplicate group carries a `class`: `app`, `test`,
`fixture`, `migration`, `generated`, or `vendored`.

- **Migrations, generated and vendored code are withheld by default.** They are
  frozen or re-copied, so consolidating one is not a refactor. Every answer
  reports what it withheld under `withheld_paths`; pass `include_ignored` to see
  them.
- **Test and fixture code is included, tagged, and kept apart.** Duplication in
  shared examples is real maintenance signal. `dupes` ranks app code first and
  then those populations; `search` discounts them (`discount` on each answer)
  rather than dropping them, so a spec that shares a name with the method it
  tests no longer outranks it.

A `mixed` duplicate group has copies in more than one class — usually a body
that exists in both `vendor/` and your own code, which is a finding about your
copy.

A repo whose layout differs says so in `.contour.toml` at its root:

```toml
[paths]
# Rules are path prefixes, exactly like a SCOPE, and beat the conventions.
test = ["engines/billing/spec"]
app = ["db/migrate"]          # these really are consolidatable here
```

## Deciding which duplicate to keep

`contour dupes --canonical` names the likely original of each group and shows
its work: git age (the **oldest surviving line** of each body, so a reformatted
implementation looks young), reference counts from trekr with its
confirmed/possible tiers, and namespace depth as a weak tiebreak. Nothing is
blended into a score.

**When it says the signals disagree, that is the answer, not a failure.** It
names which signal favours which member, and the two fail in opposite
directions: git age is fooled when an implementation is extracted to a new
home, and reference counts are fooled by a delegating shim, which is called
more precisely because it is the public front door. Read both clauses and you
can usually settle it yourself in one look.

It is off by default because it shells out — one `git blame` per body, one
trekr call per Ruby name. **Scope it.** On a directory that is seconds; on a
whole monorepo it is minutes, and every run prints what it spent.

## Feeding the index

contour indexes cheaply and eagerly (structure, hashes, names) but summaries —
the layer that makes behavioural search work — cost tokens. **Sessions that use
contour are also what fills it.** This is the deal: you read code anyway, so
say what you learned before you move on.

Three modes, cheapest first.

### 1. Grazing — while you work

When you have just read and understood a method in the course of other work,
contribute the summary before moving on. It costs you almost nothing (you
already did the reading) and it is permanent.

```
store_summary(unit: "Invoice#settle!", model: "<your model id>",
              prompt_version: "<from pending, or the version below>",
              summary: { ...the schema below... })
```

### 2. Coverage-aware fill — when starting substantive work

**Trigger:** you are about to do more than a trivial edit in an indexed repo.

**Action:** check coverage for the scope you are about to work in, and if it is
thin, fill it before diving in.

```sh
contour --status                      # is this checkout warming or none?
```

```
pending(scope: "app/models", model: "<your model id>", limit: 20)
```

Summarize what comes back and store each one. The scope you are about to work
in is the highest-value thing to fill, because you are going to read most of it
anyway — you are paying that cost either way, and this makes it stick.

### 3. Explicit fill — when asked

When someone asks you to fill or improve the index, `pending` with a larger
limit and work through it in batches. Store each summary as you finish it
rather than batching to the end, so an interrupted run keeps what it did.

### The summary schema

`pending` returns the source and the structural context for each unit. Write
the summary against those, in exactly this shape — contributions are
**validated and rejected, not repaired**:

```json
{
  "summary": "One or two sentences: what the caller gets, and what changes as a result.",
  "primary_purpose": "The single reason this method exists, as a short noun phrase.",
  "secondary_concerns": ["ranked, most significant first; usually empty"],
  "side_effects": ["persists", "network", "filesystem", "mutates", "observes", "raises", "spawns"],
  "domain": "the business area in the codebase's own words, lowercase, or \"unknown\"",
  "patterns": ["recognised implementation patterns, e.g. memoization, guard clause"]
}
```

Rules that make a summary worth storing:

- **Describe behaviour, not syntax.** "Iterates an array" is worthless.
  "Returns the unpaid invoices for a customer, newest first" is the job.
- **Do not name local variables**, and write for someone who cannot see the
  code.
- `primary_purpose` is exactly one thing. A concern is *secondary* when the
  method would still make sense without it — pagination inside a payroll query
  is secondary, the payroll is primary.
- `side_effects` is a **closed vocabulary**. Use only the seven words above; an
  invented one is rejected rather than coerced. Two that are not
  self-explanatory:
  - `raises` — it signals failure to its caller as part of its contract, rather
    than only on a bug. Ruby `raise`, **Rust `Err` return or documented
    `panic!`**, Go error return. The word is Ruby's because Ruby came first; the
    concept is not.
  - `mutates` — it changes state the caller can reach: the receiver, an
    argument, a global. Not a local.
- **Judge only from what you are shown.** Do not guess what the methods it
  calls do.

The current contributed prompt version is **`mcp-v1`**. `pending` returns the
version the server expects — pass that back verbatim, and if it does not match
this document, trust `pending` and mention the mismatch.

### When the MCP tools are not there

The same two doors exist on the command line, taking the same payload — use
them when contour has no MCP server in this session, or when its tools are
failing:

```sh
contour pending --model <your model id> --limit 20 -j   # source + context
contour store-summary <<'JSON'
{"unit": "Invoice#settle!", "model": "<your model id>", "prompt_version": "mcp-v1",
 "summary": { ...the schema above... }}
JSON
```

Note the nesting: `unit`, `model` and `prompt_version` sit *beside* `summary`,
and the six schema fields sit *inside* it. `--file` reads the payload from a
file instead of stdin.

### Why contributions are kept separate

Your summaries are stored under your model id with a provenance of `mcp`, so a
heterogeneous set of session contributions never gets mixed into a uniform
single-model fill. Both are useful; only one of them can be used to calibrate
thresholds honestly, and keeping them apart is what preserves that.

## Before the first question on a new machine

**Is `contour` on PATH?** The index takes care of itself: the first query in a
checkout indexes it, and every later one brings it up to date first.

```sh
contour --status              # what is indexed, and how much is summarized
contour index                 # only if you want the cost paid now rather than
                              # on the first question
contour embed                 # pay the whole checkout's embedding cost now,
                              # once, with progress — see "On a big repository"
```

Indexing is fast (rails: ~2 s for 3,300 files). The first `search` on a fresh
corpus embeds every identifier once and caches the result: a few seconds on a
normal repo, and **~3 minutes on something the size of rails** (54k
callables). contour prints a notice before a long pass, and refuses one
projected past five minutes — that is what `contour embed` is for.

Queries after that are served from the cache: ~0.2 s on a normal repo, and
**under a second for a scoped query on a 265k-unit corpus** — a scoped query
loads only its own scope's vectors, so the cost follows the question rather than
the repository. An *unscoped* query on that corpus is ~2 s, and it grows with
the checkout. "Instant" is true of a small repo, and of a scoped question on a
large one.

**Rust ids carry the module the file declares** — `summary::contributed::accept`,
not `accept`, and `lang::go::tests::find` rather than a `tests::find` that five
language plugins all answer to. Use the id `pending`, `--symbols` or a search hit
gives you, verbatim: it is the one `store_summary` and `similar` answer to.

Ruby gets full AST-grade normalization. Rust gets a token-stream tier that
catches copy-paste and reformatting but not renames, and says so
(`how: token_hash`). The near-duplicate tier is Ruby-only and reports how many
Rust bodies it skipped rather than staying silent about them.
