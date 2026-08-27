# Changelog

## Unreleased

Initial skeleton — nothing has been released, so everything below is new.

- **contour knows what kind of file it is looking at.** Every duplicate group,
  search hit and neighbour carries a `class`: `app`, `test`, `fixture`,
  `migration`, `generated` or `vendored`, decided from the path alone.
  - Migrations, generated and vendored code are **withheld by default** — they
    are frozen or re-copied, so consolidating one is not a refactor. Every run
    reports what it withheld and why (`1 group(s) in ignored paths withheld (1
    migration)`), and `--include-ignored` shows them.
  - Test and fixture code is **included, tagged, and ranked as its own
    population**: `dupes` puts app code first and sections the rest, and
    `search` discounts a non-app hit (disclosed as `discount`) instead of
    dropping it. Duplication in shared examples is real maintenance signal.
  - A duplicate group is withheld only when *every* copy is in an ignored path.
    One that spans classes reports as `mixed` and ranks with app code, because
    a body in both `vendor/` and `app/` is a finding about the app copy.
  - Measured: discourse exact-tier precision **0.73 → 1.00** with recall still
    1.00 (its four migration false positives are gone), and rails search top-5
    **2/77 → 12/77** (its library methods were drowning in `test/`).
- Cosines, Jaccards and floors leave the process rounded to the two decimals
  they actually have. Rounding an f32 leaves an f32, which widens back to a
  double on the way out, so `--json` and every MCP answer used to carry
  `0.44999998807907104` where the human output said `cos 0.45`.
- **A query answer is never silently stale.** Every command that answers from
  the index — `dupes`, `search`, `similar`, `summarize`, `eval`, and the MCP
  tools — brings the checkout up to date first and says so (`index refreshed —
  2 file(s)`). Delete a file and search: the method that is gone no longer
  comes back. Costs 50 ms on rails when nothing moved and 300 ms after an edit,
  against a query that takes seconds. `--status` still reports staleness rather
  than resolving it.
- **`.contour.toml`** at a checkout root states a layout the conventions get
  wrong. Rules are path prefixes — matched exactly the way a `SCOPE` is, so
  there is one path language rather than two — and they beat the conventions:
  `[paths]` with `app = ["db/migrate"]` makes migrations ordinary code again. A
  class name or rule the file gets wrong fails the run rather than being
  half-applied.

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
  Measured on rails (54,296 texts): 167 s wall against 1,192 s of cpu, which
  is 7.1x on 8 cores; the same run pinned to one worker was abandoned at a
  ten-minute timeout, so the pool is at least 3.6x. Warm searches are ~0.2 s
  on a normal repo and ~5 s on rails, where loading 54k cached vectors is most
  of the cost.
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
- **`summarize` no longer refuses every Rust unit.** The fill loop re-parses
  each body to prove it is still the one the index recorded, and did that as
  Ruby whatever the language — so every Rust unit was skipped with "the file
  changed since it was indexed", which was false and which reindexing could not
  fix. The MCP `pending` tool shared the bug, silently withholding Rust units
  from every session that asked.
- **`contour similar` refuses a name that means two units**, lists both
  locations, and accepts the `path:line` it just printed. It also no longer
  reports one unit twice under two tiers.
- **`similar` discloses what it could see** — coverage, embedder, the floor and
  what the floor withheld — and says so when nothing is similar, rather than
  exiting silently.
- **Every command's `--json` is the whole answer**, byte-for-byte what the MCP
  tool returns for the same question; a test asserts the equality. `dupes` used
  to give an agent the near tier's scale disclosure and a human a bare array.
  `-J` stays one record per line, for pipelines.
- **JSON paths are absolute**, so a consumer who is not standing in the checkout
  can resolve them. Human output shortens them back.
- **Staleness sees a working-tree edit** — the state a live session is always
  in. It was a stat of `.git/index`, which said `stale: false` while `search`
  could not find the method you had just written, and flipped to `true` on a
  commit that changed nothing contour reads. It is now the exact question, at a
  measured ~20 ms.
- **`--status` and `search` agree about coverage.** Status counted only what an
  API fill had bought, so a session's contribution left it saying `none 0/128`
  about a corpus search was already answering from. Both questions are now
  reported: what a query can answer from, and what each `(model, via)` bought.
- **The purchased half has one gate.** `summarize --fixtures` used to store an
  invented side effect permanently while the MCP path rejected the identical
  payload; the check now lives on the one door into that half. A refused answer
  is one unit's failure, not the run's.
- **A newer summary schema refuses to open**, as DEC-016 always said it would
  and did not: the upward-only migration walk let a newer marker fall through
  and be stamped back down to ours.
- Errors name the actual problem: a nonexistent path is not a repository
  question, git's `fatal:` no longer trails behind every message, the CLI lists
  the checkouts it knows when it cannot find one, a missed unit name gets
  suggestions, and a corrupt database says so once instead of twice.
- Thresholds outside 0–1 are rejected rather than silently matching nothing; an
  empty search query is rejected; a no-op reindex says "nothing new to read"
  instead of "0 units"; `--symbols` on a file with no callables says so; and the
  eval prints `n/a` rather than `NaN` for a tier with no labels.
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
