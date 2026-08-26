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
