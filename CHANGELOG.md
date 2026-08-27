# Changelog

## Unreleased

Initial skeleton — nothing has been released, so everything below is new.

- `contour index [PATH]` scans a git checkout and indexes every Ruby callable
  in it. Facts are keyed by git blob OID, so N worktrees of one repo cost one
  index and a reindex with no edits parses nothing.
- `contour --symbols FILE` outlines one file's callables, parsing it live. No
  index required, and it works in a directory contour has never seen.
- `contour --status` reports what the index holds, whether a checkout may be
  stale, and summary coverage (`none` until milestone 3).
- A method body containing `super` now has the enclosing method's name folded
  into its `norm_hash` (DEC-017). `super` dispatches by that name, so two
  identical bodies ending in `super` run different code and are no longer
  reported as clones. **Forces a reindex** — the schema version moves with it.
- Every method carries a `norm_hash`: a structural hash of its normalized
  body, with locals and positional parameters renamed to ordinals and layout,
  comments, quoting style and numeric spelling collapsed. Keyword parameter
  names are kept — they are what a caller writes.
- `contour dupes [SCOPE]` reports units whose normalized bodies are identical.
  `--min-lines` (default 4) hides bodies too short for structural identity to
  mean duplication.
- `contour summarize [SCOPE] --budget N` fills LLM summaries on demand, up to
  a budget of distinct answers. Each method gets a fine summary plus typed
  metadata: primary purpose, ranked secondary concerns, side effects, domain,
  and recognized patterns. `--fixtures FILE` replays canned answers instead,
  for offline work; `ANTHROPIC_API_KEY` is needed only for a live fill, never
  to build or test.
- `contour --status` reports summary coverage per model — `complete`,
  `warming`, or `none`, with the fraction beside it.
- Summaries are stored under `norm_hash + ctx_hash + prompt_version + model`,
  so a rename, a move, or a reformat never re-summarizes, and switching models
  builds an adjacent set rather than overwriting one.
- **Summaries survive a reindex.** The derived tables are still dropped and
  rebuilt on any schema change, but summaries are purchased work that cannot
  be recomputed from local bytes, so they live under their own version and are
  never dropped automatically.
- `contour search "english query"` ranks callables by fusing a name match with
  a meaning match (RRF, K=60). Every answer discloses which half found each
  hit, the cosine where the semantic half contributed, how much of the corpus
  is summarized, and which embedder answered.
- `contour similar Owner#method` lists nearest neighbours with the tier that
  found each: `structural` for an identical normalized body (which carries the
  body size as evidence, not a manufactured confidence) and `semantic` for a
  nearby summary (which carries the cosine, because that judgment is graded).
- Summaries are embedded to 256-dim vectors. The default build uses a
  deterministic hash embedder — enough to exercise the pipeline, and offline —
  while `--features semantic` (or `semantic-dynamic`) enables a local
  all-MiniLM-L6-v2 through ONNX Runtime. The embedder kind and model are part
  of every vector's key, so switching costs a re-embed, never a corruption.
- **Rust is the second language** (DEC-012). `index`, `--symbols`, `dupes`,
  `search`, and `similar` all work on `.rs` files, and units from both
  languages share one store. Names render in each language's own dialect:
  `Widget#save` in Ruby, `Widget::run` in Rust.
- Rust normalization is a deliberately degraded tier: a comment-stripped token
  stream, disclosed as `token_hash` and never as `structural`. Reformatting
  and re-commenting collide; a renamed local does not, where Ruby's would.
- **`contour mcp`** serves the Model Context Protocol over stdio: `search`,
  `similar`, `dupes`, `symbols`, `status`, `index`, plus `pending` and
  `store_summary`. Tool results are byte-for-byte the JSON `--json` returns, so
  no disclosure field can exist for a human and go missing for an agent.
- **Sessions can feed the index.** `store_summary` accepts a summary a session
  wrote, validated against the same schema the API path uses and refused rather
  than repaired; `pending` lists what still needs one, with source and context.
  Contributions are keyed by the contributing model with `via = mcp`, so they
  never mix into a uniform fill (DEC-018). **Migrates the summary table** — the
  first migration of the purchased half, rather than a drop.
- The cold embed pass uses one embedder per worker thread, so a corpus warms
  in parallel rather than serially through a single mutexed ONNX session.
  Searches after the first are ~0.2 s.
- ONNX embedders now report which model they are, so two different models can
  no longer share a vector cache key (DEC-005 said they must not; the default
  `model()` had quietly made them).
- **An identifier tier makes search work with zero LLM spend.** Every unit's
  humanized name, owner and parameters are embedded locally, so a fresh
  checkout is searchable in English immediately. Summaries are now an upgrade
  rather than an entry fee, and each hit says which tier answered
  (`semantic_via`: `summary` or `identifier`).
- A Claude skill and MCP wiring instructions live in `claude/`.
- `contour --symbols --json` now includes each unit's `id`, the same handle
  every other command prints and `similar` accepts.
- **`contour dupes --near`** reports bodies that are nearly the same shape,
  with the measured Jaccard on each. Similarity is computed over subtree
  signatures collected during normalization; candidates come from an inverted
  index, so nothing is compared pairwise — on rails that is 56,706 pairs
  scored out of 206,075,451 possible, in 0.7s.
- `contour similar` gains its missing `near_structural` tier, between exact
  identity and meaning.
- The near threshold (jaccard 0.80) is **the first threshold measured on
  contour's own corpus** rather than inherited: labeled distinct pairs top out
  at 0.667, labeled near-duplicates bottom out at 0.905, and it sits in the
  gap. The eval asserts both edges.
- Ruby only. Rust stays on the exact tier, and `--near` says so rather than
  returning a silence that looks like "nothing found".
- **Forces a reindex**: signatures are new, and the fold that produces
  `norm_hash` changed shape to compute them.
- `contour similar UNIT [PATH]` can be pointed at a checkout, like `index` and
  `search` already could, and failing to find one now leads with what to do
  about it rather than with git's own `fatal:`.
- A canonical pick names a **location**, not just an id. rq holds five
  `tests::find`, one per language plugin: naming the winner by id there says
  nothing, and two signals favouring two different members would have compared
  equal and been reported as agreeing. Human output marks the winner in the
  member list with `*`.
- `contour eval` scores canonicality against `canonical.tsv`, per edge and per
  signal, counting an abstention apart from a wrong answer. On the rails
  labels the two deciding signals fail in **opposite** directions: git age is
  fooled when an implementation is extracted to a new home, and reference
  counts are fooled by a delegating shim, which is called more precisely
  because it is the front door. Neither dominates, and the run reports which
  signal favours which member on every disagreement.
- **`dupes` groups are ordered by what consolidating each would buy**, across
  both tiers at once: the copies beyond the first, times the body's node count,
  discounted by the near tier's measured Jaccard. Every component is printed
  beside the estimate (`~191 nodes · 2 × (33 lines, 191 nodes) [structural]`),
  so the order can be argued with rather than trusted. An exact group beats a
  near one of the same size and a big near pair still beats a small exact one,
  with no weight anywhere saying so.
- The estimate counts **nodes, not lines**, and the choice was measured rather
  than argued: on rails the two correlate at only 0.79 and their orderings
  share 10 of their top 20 groups. Lines overstate a heredoc by 16x (one
  83-line method is five nodes), which inflates exactly the duplications least
  worth acting on; and an order that moves when somebody runs a formatter
  contradicts a tool whose premise is that a reformat is not a change. Lines
  are still printed, because a person feels lines.
- **Forces a reindex**: every unit now carries the node count of its normalized
  body, which the fold was already computing and discarding.
- **`contour dupes --canonical`** names the likely-original member of every
  group and says why: git age (the oldest surviving line of each body),
  reference counts from `trekr --refs` with its confirmed/possible tiers, and
  namespace depth as a weak tiebreak. Each signal reports its own pick and its
  own measurement; nothing is blended into a score. **When the signals disagree
  there is no pick** — that usually means the older one was superseded and never
  deleted, which is a finding rather than a failure.
- A signal that cannot be measured says so and why — "trekr reads Ruby, and
  this is rust", or trekr's own "never been indexed" with the command that
  would fix it — and never degrades into a guess.
- Off by default: it is the only part of `dupes` that leaves the process, at
  one `git blame` per body and one `trekr` call per Ruby name. A rails scope is
  seconds; the whole of rails is minutes, and every run prints what it spent.
  `$CONTOUR_TREKR` names the binary, so a machine without trekr just reports
  the signal absent.
- The MCP `dupes` tool takes `canonical` too, and returns byte-for-byte what
  `--json` does.
- The near tier's skip disclosure no longer blames Rust for a Ruby body. A
  body with no comparable sub-shape is skipped for one of two reasons — its
  language has none (Rust), or every sub-shape in it fell below the size floor
  — and `near_stats` now counts and names them separately.
- `contour eval <SET>` scores a checkout against a labeled set: search hit-rate
  (top-1 / top-5 / found) for contour and two baselines, duplicate precision
  and recall, and the cosine distributions that say where the relevance floor
  and `--min-lines` belong. Runs with no floor, deliberately — calibrating a
  threshold against results it already filtered can only confirm it.
- A labeled fixture set ships in-repo (`tests/eval/fixture`) and runs in CI.
  The labels are a draft awaiting review; see `tests/eval/README.md`.
- Index lives at `~/.local/share/contour/contour.db`; `$CONTOUR_DB` overrides
  it. There are no migrations — a schema-version mismatch drops the database
  and reindexes, because it is a cache of a pure function, not a system of
  record.
