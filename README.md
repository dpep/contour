# contour

A multi-resolution semantic index of source code — search, navigation, duplicate
detection, and architecture analysis over *intent* rather than syntax.

> "Which methods retrieve collections of domain objects?"
> "Where is the canonical implementation of this behavior?"
> "Which methods are doing essentially the same thing?"

Traditional tools index what code *says*; contour indexes what code *means*.
Each callable is progressively compressed — AST → normalized AST → structural
hash → LLM behavior summary → embedding — and every layer serves different
questions, from exact clone detection to English-language concept search.

## Status

Early. Ruby only. The pipeline is being built a layer at a time; what works
today is the cheap end of it.

```sh
contour index [PATH]     # scan a checkout, index every callable in it
contour dupes [SCOPE]    # units whose normalized bodies are identical
contour summarize [SCOPE] --budget N   # fill LLM summaries, on demand
contour --symbols FILE   # outline one file — parses it live, no index needed
contour --status         # what the index holds, and what it is missing
```

Not built yet: `search`, `similar`.

## How it is put together

One noun: a **unit** is one callable span of source. Everything contour does is
a lookup or a join over the chain of keys a unit accumulates.

| key | costs | answers |
| --- | ----- | ------- |
| blob OID | a git call | where it is |
| structural hash | a parse | what shape it is — exact clones |
| summary | an LLM call | what it means |
| embedding | an inference | where it sits in meaning-space |

Cheap layers index eagerly, repo-wide. Expensive layers fill on demand and
budgeted, with coverage disclosed on every answer.

The index is per-machine (`~/.local/share/contour/contour.db`, `$CONTOUR_DB`
overrides) and keyed by git blob OID, so branch switches, rebases, and N
worktrees of one repo cost nothing.

See [docs/PLAN.md](docs/PLAN.md), [docs/DECISIONS.md](docs/DECISIONS.md), and
[docs/PRIOR-ART.md](docs/PRIOR-ART.md).

## Prior art

Roughly 70% of the plumbing is borrowed from sibling tools by the same author —
Prism extraction and the blob store from
[trekr](https://github.com/dpep/trekr), the Prism node tables from
[rwr](https://github.com/dpep/rwr), the embedding and vector-cache design from
[gqls](https://github.com/dpep/gqls). Vendored files carry a provenance header
and stay close enough to upstream that a sync is a re-copy.
