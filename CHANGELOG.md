# Changelog

## 0.3.0 — 2026-09-01

- **`contour embed [SCOPE]` pays the embedding bill on purpose.** A query
  embeds whatever in its scope has no vector yet, and `search`/`similar` refuse
  a cold scope projected past five minutes rather than running it (0.2.0). This
  is the way to say yes to it: it embeds only what has none, commits every
  batch, prints progress with its own measured rate and an ETA, and continues
  where an interrupted run stopped — so **run it once per machine and every
  query over that scope is warm afterwards** (vectors are keyed by content and
  shared across checkouts). `--budget SECONDS` bounds a sitting; `--json` /
  `--ndjson` report what it did. Measured on a synthetic 265k-unit corpus: 943 s
  and 851 MB, against 1,219 MB for the query that embeds the same corpus.
  Deliberately **not** an MCP tool — a two-hour tool call is the problem it
  solves — and the refusal message now names it. DEC-034.

- **A scoped query no longer pays for the whole repository's vectors.** Loading
  them read the entire vector table whatever scope you asked about — about a
  second and 750 MB at 132k units, and the whole cost of every warm call at
  monorepo scale. It now reads only the vectors the scope needs. **Nothing to
  do**: no reindex, no re-embed, same answers. `docs/PLAN.md` has the
  before/after at two corpus sizes. DEC-033.

## 0.2.0 — 2026-08-31

- **`similar` takes a scope.** Its second positional is now a path to seek
  neighbours *within*, exactly as `search` and `dupes` read theirs, and the MCP
  tool gained a matching `scope` property. **This changes what an existing
  invocation means**: `contour similar Owner#method some/dir` used to search the
  whole checkout and now searches `some/dir`, and so does a bare `contour
  similar Owner#method` run from inside a subdirectory — which is how `search`
  and `dupes` have always behaved. Pass the checkout root to search all of it.
  Every answer now carries the `scope` it searched, so a short list from one
  directory cannot read as a thin corpus. Scope it on a large repository:
  everything in scope needs a vector, and anything without one is embedded on
  the spot (DEC-030).

- **The MCP server answers more than one call at a time, and a cancelled call
  stops.** It reads on one thread and works on four, so a `symbols` call no
  longer waits behind a heavy one — a field report watched that wait run to
  fifteen minutes. `notifications/cancelled` now reaches the running request
  and stops it, and **so does hanging up**: closing the server's stdin cancels
  everything outstanding, which is what the same report needed when abandoning
  a call left ten cores burning until the process was killed by hand.
  Responses now come back in whatever order they finish rather than in the
  order they were asked — which JSON-RPC allows and clients match by `id`, but
  a hand-rolled client that assumed otherwise must stop assuming it. The
  mid-session restart (DEC-025) is unaffected and still exec's only when
  nothing is outstanding. DEC-031.
- **A query that would have to embed the whole corpus now says so first.**
  `search` and `similar` refuse a cold scope whose embedding is projected past
  five minutes, naming the units in scope, how many have no vector, the rate
  and thread count behind the projection, and what it would cost — rather than
  starting a two-hour run with nothing to show for it. **Scope it**: a
  directory at a time embeds the same corpus in the same total time, keeps what
  it embeds, and answers at every step. `CONTOUR_EMBED_BUDGET` sets the budget
  in seconds and `0` removes it. Builds with the hash embedder are ~8.5 million
  texts a second and will not meet this. DEC-032.
- **Where a monorepo's time actually goes, measured** at 132k and 256k units
  and written down in `docs/PLAN.md`: a cold unscoped query is an embedding run
  at ~295 units/second, 97% of the wall clock, extrapolating to about 110
  minutes and 9 GB of memory at 2M units. The cosine scan the plan worried
  about is not the problem at 5× the size it was worried about.

## 0.1.0 — 2026-08-31

First release. contour indexes what code *means*: every callable in a checkout
is compressed through a chain of layers — parse, normalize, structural hash,
LLM behaviour summary, embedding — and each layer answers a different question,
from exact clone detection to English-language search. Ruby and Rust.

- **Install it with a real embedder.** `brew install dpep/tools/contour` builds
  the `semantic-dynamic` feature against the `onnxruntime` keg. From cargo the
  crate is `contour-index` — `contour` was taken — and the feature is yours to
  pick: `cargo install contour-index --features semantic`. **With neither
  feature contour still runs**, on a deterministic hash embedder that exercises
  the whole pipeline offline but matches what code is *called*, not what it
  does. `contour --version` says which embedder it was built with, `--status`
  says it again, and `search` says which one actually answered — a
  `semantic-dynamic` build falls back to the hash embedder if it finds no
  system ONNX Runtime.
- **Ask in English.** `contour search "settle an invoice"` ranks by a name match
  fused with a meaning match, and every hit shows both halves of its evidence
  and which tier answered it. A unit's name, owner and parameters are embedded
  locally, so a fresh checkout is searchable before anything has been bought —
  LLM summaries are an upgrade, not an entry fee.
- **Find the duplication a rename or a reformat hides.** `contour dupes` groups
  identical bodies — Ruby normalization is AST-grade, so a rename or a reformat
  is not a change — ordered by what consolidating each would buy. `--near`
  reports nearly-identical bodies with the measured Jaccard on each, and
  `--canonical` names the likely original from git age and reference counts,
  saying nothing when its signals disagree, because a disagreement is itself a
  finding. `contour similar Owner#method` walks out from one unit instead.
- **Outline a file without indexing anything.** `contour --symbols FILE` parses
  it live and marks everything that is not public.
- **Summaries are bought on demand, and never twice.** `contour summarize
  --budget N` fills them through the Anthropic API (`ANTHROPIC_API_KEY`), keyed
  so that nothing a model was never shown can move a cache key. A session can
  contribute rather than pay: `contour pending` lists what still needs one and
  `contour store-summary` takes it as JSON.
- **`contour mcp` serves the whole tool to an agent** over stdio — `search`,
  `similar`, `dupes`, `symbols`, `status`, `index`, `pending`, `store_summary`.
  Tool results are byte-for-byte what `--json` returns, so no disclosure can
  exist for a human and go missing for an agent, and they are compact rather
  than pretty-printed, because a tool result is read by a model. A resident
  server re-execs itself when the binary underneath it is replaced, so
  upgrading contour mid-session does not strand the session.
- **The index is a cache, not a system of record.** It lives at
  `~/.local/share/contour/contour.db` (`$CONTOUR_DB` overrides) and is keyed by
  git blob OID, so branch switches, rebases and N worktrees of one repo cost
  nothing. Derived data sits beside it in `contour-derived-v12.db`, named for
  its schema version: a contour whose schema differs builds its own file, in
  seconds, and can neither drop nor be refused by anyone else's. **Nothing to
  do on an upgrade** — and nothing to lose, because summaries live in the
  unversioned half and are never dropped.
- **What kind of file it is comes from the path, never the bytes** — `app`,
  `test`, `fixture`, `migration`, `generated`, `vendored`. Migrations, generated
  and vendored code are withheld by default and every run says how many
  (`--include-ignored` shows them); test code is included, tagged, and ranked as
  its own population, because duplication in shared examples is real
  maintenance signal. A repo whose layout differs states it in `.contour.toml`
  at its root.
- **Every answer discloses what it could see**: the embedder, summary coverage,
  the tier that answered, and what was skipped and why. `contour eval <SET>`
  scores a checkout against a labeled set — hit-rate against two baselines,
  duplicate precision and recall — and a labeled fixture set ships in-repo.

Rust is a deliberately degraded tier: a comment-stripped token stream,
disclosed as `token_hash` and never as `structural`. It catches copy-paste and
stops there, and `--near` says so rather than returning a silence that looks
like "nothing found".
