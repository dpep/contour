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
contour similar 'Invoice#settle!'                    # does this already exist?
contour dupes --near                                 # copy-paste, incl. tweaked
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

`similar` discloses a tier per neighbour: `structural` (identical normalized
body), `near_structural` (mostly the same shape, with a measured Jaccard), or
`semantic` (a nearby summary, with the cosine). A `structural` match is a fact;
a `semantic` one is a suggestion.

If `search` returns less than you expect, run `contour --status` before
concluding the code is not there.

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
  invented one is rejected rather than coerced.
- **Judge only from what you are shown.** Do not guess what the methods it
  calls do.

The current contributed prompt version is **`mcp-v1`**. `pending` returns the
version the server expects — pass that back verbatim, and if it does not match
this document, trust `pending` and mention the mismatch.

### Why contributions are kept separate

Your summaries are stored under your model id with a provenance of `mcp`, so a
heterogeneous set of session contributions never gets mixed into a uniform
single-model fill. Both are useful; only one of them can be used to calibrate
thresholds honestly, and keeping them apart is what preserves that.

## Before the first question on a new machine

**Is `contour` on PATH, and is the repo indexed?**

```sh
contour --status              # nothing listed? index it
contour index                 # cheap and idempotent; a no-op reindex parses nothing
```

Indexing is fast (rails: ~3 s for 3,300 files). The first `search` on a fresh
corpus embeds every identifier once and caches the result. With the ONNX
embedder that is quick on a normal repo (~530 units in ~3 s) but **slow on a
very large one** — rails, at 54k units, takes many minutes. contour prints a
notice before a long pass. If you hit it, either let it warm once or use a
`scope` to bound the work.

Ruby gets full AST-grade normalization. Rust gets a token-stream tier that
catches copy-paste and reformatting but not renames, and says so
(`how: token_hash`). The near-duplicate tier is Ruby-only and reports how many
Rust bodies it skipped rather than staying silent about them.
