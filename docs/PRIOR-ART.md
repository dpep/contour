# Prior art

What contour draws from the sibling repos in `~/code/lib/rust/`, mapped from
an exploration of all of them (2026-08). Roughly 70% of the plumbing exists;
the greenfield part is the LLM summarization layer (no repo in the tap makes
an LLM API call today) plus clustering and the eval harness.

## gqls — the architectural template

Closest analogue: record → text → embedding → hybrid search. Fork/adapt:

- `src/semantic/cache.rs` — content-keyed incremental vector cache. Cache
  key derives from the exact text embedded (key can never drift from
  content); donor-file reuse (one changed record = one inference);
  config-versioned keys; atomic write-then-rename; LRU on files *and*
  bytes; vendored stable FNV-1a because `DefaultHasher` changes across Rust
  releases (with a frozen-key regression test).
- `src/semantic/{embed.rs, embed/onnx.rs, mrl.rs}` — `Embedder` trait,
  ONNX MiniLM + hash fallback, 256-dim MRL truncation, `Workload`
  enum (bulk inference must use 1 intra-thread under rayon — a measured
  19% regression otherwise), thread-local embedder pool.
- Relevance-floor calibration (`src/semantic/mod.rs:30-54`) — the measured
  table showing absolute thresholds need 256 dims. Rerun this experiment
  for method summaries before trusting any duplicate threshold.
- RRF fusion (`combine()` in `src/cli.rs`, K=60) — lexical ⊕ semantic.
- Background auto-warm with single-flight mtime-TTL locking; `humanize()`
  identifier splitting before embedding (snake_case needs it too);
  `src/logging.rs`, `src/profile.rs`, `src/paths.rs`; the directory-walk
  skip lists; `script/check.sh` / `script/bench.sh`.

## trekr — the parsing foundation

- **Vendor `src/core/mod.rs` + `src/extract/*`** (~2,750 lines, deps:
  ruby-prism + serde). `Def` carries name, kind, lexical nesting,
  singleton, visibility, params, `via` (which macro generated it),
  positions. Rails macro expansion done (`has_many`, `scope`, `delegate`,
  `attr_*`, `define_method`, …; literal names only — refuses rather than
  guesses).
- **Blob-OID store design** (`src/scan/mod.rs`, `src/store/`): facts keyed
  by git blob SHA-1, diffed against `git ls-files -s`; hash uncommitted
  files the way git does so they key identically once committed;
  `git_fingerprint` O(1) staleness probe; schema version bump =
  drop-and-rebuild, no migrations (the DB is a cache of a pure function).
- `Facts::surface()` — FNV structural digest of defs+ancestry; the shape of
  our normalized-body hash (ours strips positions; its tests are the model).
- `Tree` (`src/tree/mod.rs`) — resolved ownership (`MethodDef { name,
  owner, singleton, … }`), memoized ancestor linearization with unresolved
  lists, rebuilt-not-patched (~120ms on rails). Phase 3.
- Disclosure contract (`src/resolve/`): status/confidence/how, `--explain`
  renders the same JSON. The `tests/testbed/` file-driven harness.
- Gems via direct `Gemfile.lock` parsing; `docs/DECISIONS.md` DEC-record
  convention.

## rwr — normalization

- **`src/pattern/generated.rs`** (mechanically generated from Prism's node
  schema by `script/gen-compare.py`) — location-free
  `(discriminant, atoms, children)` decomposition of every Prism node,
  literals normalized (`"x" == 'x'`, `1_000 == 1000`). Our structural hash
  is this table folded into a hasher instead of compared for equality.
- `src/pattern/compare.rs` (`node_eq`) — the recursion to mirror; its tests
  pin what is and isn't structure.
- `src/pattern/matcher.rs` `Bound`/`Env` — the binding discipline for
  ordinal variable renaming.
- Hierarchy rebuilt per run, never persisted (<200ms full rails parse) —
  the precedent for which layers deserve no cache.

## ae

- Running-mean centroid embeddings (`Store::update_candidate_context`) —
  one incrementally-updated vector per entity; useful for class/file/
  cluster centroids.
- `src/ipc.rs` — the warm-model daemon (frame protocol, flock leader
  election, bounded both directions, idle janitor), if model-load latency
  ever warrants one. Its CHANGELOG catalogues the failure modes.
- `hf-hub` fetch with timeout wrapper (hf-hub itself has none).

## rq

- Multi-language `LanguagePlugin` registry — the shape for post-Ruby
  expansion (though its Ruby extraction is weaker than trekr's).
- `index_budgeted` + `coverage ∈ {complete, warming, none}` — the
  on-demand/budgeted fill pattern with coverage disclosure (DEC-009).
- FTS5 trigram over names — a candidate for the lexical half of RRF.

## pattern_engine

Not relevant to the pipeline; the house example of a lib-shaped crate
(genuinely `pub` modules), which contour's crate layout follows (DEC-002).
