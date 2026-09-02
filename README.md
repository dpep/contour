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

Early. Ruby and Rust. The pipeline is being built a layer at a time.

Ruby normalization is AST-grade — a rename or a reformat is not a change. Rust
is a deliberately degraded tier (a comment-stripped token stream, disclosed as
`token_hash`), which catches copy-paste and stops there.

```sh
contour index [PATH]     # scan a checkout, index every callable in it
contour dupes [SCOPE]    # identical bodies; --near for nearly-identical ones
                         # `duplicates` is the same command, spelled out
                         # --canonical names the likely original, with the basis
contour embed [SCOPE] [--budget SEC]   # fill embeddings, so queries answer warm
contour summarize [SCOPE] --budget N   # fill LLM summaries, on demand
contour pending [SCOPE] --model M      # what nothing has summarized yet
contour store-summary [PATH]           # contribute one, as JSON on stdin
contour search "english query"         # rank by name match + meaning match
contour similar Owner#method [SCOPE]   # nearest neighbours, tier disclosed
contour eval <SET>                     # score against a labeled set
contour mcp                            # MCP over stdio, for an agent client
contour --symbols FILE   # outline one file — parses it live, no index needed
contour --status [PATH]  # what the index holds, and what it is missing
contour … --profile      # on any command: where this run's wall clock went
```

## Install

```sh
brew install dpep/tools/contour
# from source (the crate is `contour-index`; `contour` was taken):
cargo install contour-index --features semantic
```

Brew builds the `semantic-dynamic` feature, which dlopens the `onnxruntime`
keg, so English search answers on meaning. A build with neither feature falls
back to a deterministic hash embedder that exercises the pipeline offline but
matches what code is *called* — `contour --version` says which one you have.
See [claude/INSTALL.md](claude/INSTALL.md) to wire it into Claude Code.

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

A query embeds whatever in its scope has no vector yet, which is a wait a large
repository cannot absorb: at the measured rate a million units is over an hour.
So `search` and `similar` **refuse** a cold scope projected past five minutes
and say what it would cost (`CONTOUR_EMBED_BUDGET` overrides), and `contour
embed` is where you pay it on purpose — scoped, resumable, with progress on
stderr. Embeddings are keyed by content and shared across checkouts, so a
machine pays once.

The index is per-machine (`~/.local/share/contour/contour.db`, `$CONTOUR_DB`
overrides) and keyed by git blob OID, so branch switches, rebases, and N
worktrees of one repo cost nothing.

## What kind of file it is

A path has a class — `app`, `test`, `fixture`, `migration`, `generated`,
`vendored` — decided at the file layer from the path alone, never from the
bytes (DEC-021, DEC-022). Migrations, generated and vendored code are frozen or
re-copied rather than consolidated, so they are withheld by default and every
run says how many it withheld; `--include-ignored` shows them. Test and fixture
code is *included*, tagged, and ranked as its own population — duplication in
shared examples is real maintenance signal, and `search` discounts it rather
than dropping it.

A repository whose layout differs says so at its root, in rules that are path
prefixes matched exactly the way a `SCOPE` is:

```toml
# .contour.toml
[paths]
test = ["engines/billing/spec"]
app = ["db/migrate"]      # these really are consolidatable here
```

See [docs/PLAN.md](docs/PLAN.md), [docs/DECISIONS.md](docs/DECISIONS.md), and
[docs/PRIOR-ART.md](docs/PRIOR-ART.md).

## Prior art

Roughly 70% of the plumbing is borrowed from sibling tools by the same author —
Prism extraction and the blob store from
[trekr](https://github.com/dpep/trekr), the Prism node tables from
[rwr](https://github.com/dpep/rwr), the embedding and vector-cache design from
[gqls](https://github.com/dpep/gqls). Vendored files carry a provenance header
and stay close enough to upstream that a sync is a re-copy.
