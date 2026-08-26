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
- Index lives at `~/.local/share/contour/contour.db`; `$CONTOUR_DB` overrides
  it. There are no migrations — a schema-version mismatch drops the database
  and reindexes, because it is a cache of a pure function, not a system of
  record.
