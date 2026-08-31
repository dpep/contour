# Changelog

## Unreleased

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
