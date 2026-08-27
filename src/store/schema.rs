//! The on-disk schema, in two halves with different rules.
//!
//! **Derived tables** (`blob`, `unit`, `checkout`, `file`) are a cache of a
//! pure function of bytes this machine can read again. A version mismatch
//! drops and rebuilds them — seconds of work, and no migration bugs ever.
//!
//! **The summary table is not that.** It holds work that was *purchased*: a
//! full fill of a large repo is hundreds of dollars of LLM calls, and it
//! cannot be recomputed from local bytes. Dropping it on an extractor tweak
//! would be indefensible, so `VERSION` does not govern it — it has its own
//! version, its own tripwire, and it survives every rebuild of the tables
//! above. It is keyed entirely by content hashes and has no foreign keys into
//! them, so nothing about a rebuild can leave it inconsistent.

/// Bump on any change to the derived schema **or to what the extractor
/// emits**.
///
/// The second half is easy to miss: units are cached by blob OID on the
/// premise that they are a pure function of the bytes — but when the
/// *function* changes, identical bytes must still be re-read. An extractor fix
/// otherwise ships silently dead, because every blob it would affect is
/// already "known".
///
/// There are no migrations, and that is deliberate (DEC-003): everything under
/// this version is derived from bytes this machine can read again. A mismatch
/// drops it and reindexes — which costs seconds and removes an entire class of
/// migration bug. It also makes adding a column free, so nothing needs to be
/// carried speculatively. See the module header for what this version
/// deliberately does *not* govern.
pub(crate) const VERSION: i64 = 10;

/// Applied whole to a fresh database.
pub(crate) const SCHEMA: &str = r#"
-- ── Layer 1: a pure function of a blob's bytes ───────────────────────────
-- Nothing above `file` may mention a path, a checkout, or a repository. That
-- restraint is what makes N worktrees of one repo cost one index, and what
-- lets a summary survive a file being renamed or moved.

CREATE TABLE blob (
  id           INTEGER PRIMARY KEY,
  oid          TEXT    NOT NULL UNIQUE,   -- git blob sha1
  lines        INTEGER NOT NULL,
  parse_errors INTEGER NOT NULL
);

-- One callable. `via` names the macro that generated it (`attr_reader`,
-- `scope`, …) and is NULL for a literal `def`; a macro-generated unit has no
-- body, hence no norm_hash and nothing to summarize.
CREATE TABLE unit (
  blob_id   INTEGER NOT NULL REFERENCES blob(id) ON DELETE CASCADE,
  lang      TEXT    NOT NULL,             -- ruby | rust
  name      TEXT    NOT NULL,
  owner     TEXT    NOT NULL,             -- lexical namespace, '::'-joined
  singleton INTEGER NOT NULL,             -- `def self.x` / inside `class << self`
  params    TEXT    NOT NULL,             -- 'req:a;key:b', Ruby's vocabulary
  via       TEXT,
  line      INTEGER NOT NULL,
  end_line  INTEGER NOT NULL,
  -- Hash of the normalized body: locals renamed to ordinals, layout and
  -- comments collapsed (see `ruby::norm`). NULL when there is no body — a
  -- macro-generated unit has nothing to hash and nothing to summarize.
  -- This is the key expensive layers hang off (DEC-003), so it is an on-disk
  -- format: see `crate::hash` for why it is not a `DefaultHasher`.
  -- Seeded with the language, so two languages sharing this column cannot
  -- collide into one another's hash space.
  norm_hash INTEGER,
  -- Nodes in that normalized body. A size that survives a reformat, where
  -- `end_line - line` does not, and free to compute: the fold already counts
  -- every node to apply its subtree floor. NULL for Rust, whose token-stream
  -- tier has no AST count to give (DEC-012).
  nodes     INTEGER
);

-- ── Layer 2: the path→blob map, the only place a path appears ────────────

CREATE TABLE checkout (
  id         INTEGER PRIMARY KEY,
  root       TEXT    NOT NULL UNIQUE,     -- absolute worktree path
  indexed_at INTEGER NOT NULL,            -- unix seconds
  -- The file map folded into one number: the sum over files of
  -- hash(path) ^ hash(blob oid). An identical key means an identical map, so
  -- the rewrite below is skipped outright — which is the whole cost of a
  -- no-op reindex at scale.
  -- It is also the answer to "is this checkout stale": recomputing it costs
  -- one `git ls-files` (~20 ms on rails) and is exact, where the stat of
  -- `.git/index` this replaced was O(1) and blind to every uncommitted edit.
  map_key    INTEGER NOT NULL
);

CREATE TABLE file (
  checkout_id INTEGER NOT NULL REFERENCES checkout(id) ON DELETE CASCADE,
  path        TEXT    NOT NULL,           -- relative to the checkout root
  blob_id     INTEGER NOT NULL REFERENCES blob(id),
  PRIMARY KEY (checkout_id, path)
) WITHOUT ROWID;

-- One embedding, keyed by the exact text it came from plus everything else
-- that invalidates it (width, embedder kind, model — see `embed::config_key`).
-- Derived rather than purchased: re-embedding a large repo locally is about a
-- minute, so DEC-016 puts it on this side of the line. An API-backed embedder
-- would move it to the other side.
CREATE TABLE vector (
  config_key INTEGER NOT NULL,
  text_key   INTEGER NOT NULL,
  vec        BLOB    NOT NULL,   -- dims x f32, little-endian
  PRIMARY KEY (config_key, text_key)
) WITHOUT ROWID;

-- The sub-shapes inside one normalized body, for the near-structural tier.
-- Keyed by `norm_hash` rather than by unit because it is a pure function of
-- the body: a clone at ten places is one signature. Derived, so it is rebuilt
-- freely with the tables above (DEC-016).
--
-- `nodes` and `parent_hash` are what make the tier's measure node-denominated
-- rather than shape-denominated: a shape's size is what consolidating it buys,
-- and its parent says whether a larger shared shape already counted those same
-- nodes. Counting shapes instead makes one edit look like many, because a
-- Merkle fold invalidates every shape above the edit (see `crate::near`).
CREATE TABLE signature (
  norm_hash    INTEGER NOT NULL,
  subtree_hash INTEGER NOT NULL,
  nodes        INTEGER NOT NULL,
  parent_hash  INTEGER NOT NULL,   -- 0 at the body root
  PRIMARY KEY (norm_hash, subtree_hash)
) WITHOUT ROWID;

CREATE INDEX signature_subtree ON signature(subtree_hash);

CREATE INDEX unit_name ON unit(name);
CREATE INDEX unit_norm ON unit(norm_hash);
CREATE INDEX unit_blob ON unit(blob_id);
CREATE INDEX file_blob ON file(blob_id);
"#;

/// Every **derived** table, dependents first, so a drop respects nothing.
/// `summary` is deliberately absent: see the module header.
pub(crate) const TABLES: [&str; 6] = ["signature", "vector", "file", "checkout", "unit", "blob"];

/// Bump only for a real change to the summary table.
///
/// Unlike `VERSION`, this one cannot be answered by recomputing — a mismatch
/// is a migration or an admission of data loss, so `Store::init` refuses to
/// open rather than guessing. That refusal is the point: the first time this
/// number ever changes, it must not silently destroy paid work.
pub(crate) const SUMMARY_VERSION: i64 = 2;

/// The expensive layer. Applied on every open, dropped by nothing.
pub(crate) const SUMMARY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;

-- One LLM answer. The primary key IS the cache key (DEC-003), and every part
-- of it is something that changes the answer:
--
--   norm_hash  the normalized body. Deliberately blind to local names, which
--              is what makes a rename free.
--   ctx_hash   a hash of the exact context text the prompt renders, so the
--              key cannot drift from what the model was actually told.
--   prompt     the prompt version, pinned by a frozen test over the whole
--              request shape — instructions, schema, effort, token ceiling.
--   model      so summaries from different models sit side by side rather
--              than overwriting each other.
--   via        how the answer arrived: 'api' (contour called a model) or
--              'mcp' (a session handed one over). In the key, not beside it,
--              so a heterogeneous set of session contributions can never be
--              mistaken for a uniform single-model fill — which is what the
--              Phase 1 calibration still needs (DEC-018).
--   variant    DEC-008: 'body' reads code only, 'commented' reads the
--              comments too. Only 'body' is generated today; comparing a
--              comment against a summary that read the comment is circular,
--              so drift detection needs the honest one to exist separately.
--   level      DEC-013: 'unit' today; container rollups later.
--
-- `variant` and `level` are carried before they are used, against the usual
-- rule, because they are KEY components. Adding a key column later re-keys
-- every stored row — and re-keying summaries means buying them all again.
CREATE TABLE IF NOT EXISTS summary (
  norm_hash  INTEGER NOT NULL,
  ctx_hash   INTEGER NOT NULL,
  prompt     TEXT    NOT NULL,
  model      TEXT    NOT NULL,
  via        TEXT    NOT NULL,
  variant    TEXT    NOT NULL,
  level      TEXT    NOT NULL,
  json       TEXT    NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (norm_hash, ctx_hash, prompt, model, via, variant, level)
) WITHOUT ROWID;
"#;

/// Migrations for the purchased half, applied in order from the stored
/// version.
///
/// DEC-016 says a mismatch here is "a migration someone has to write, or an
/// admission of data loss someone has to make on purpose". This is the first
/// one, and writing it rather than letting the tripwire fire is the whole
/// point of having said that: summaries cost money or somebody's attention,
/// and neither is recoverable by reindexing.
///
/// SQLite cannot alter a primary key, so adding `via` is a table rebuild.
/// Existing rows backfill to `api`, which is honest — everything stored before
/// the contribution path existed came through it.
pub(crate) const SUMMARY_MIGRATIONS: [(i64, &str); 1] = [(
    1,
    r#"
    ALTER TABLE summary RENAME TO summary_v1;
    CREATE TABLE summary (
      norm_hash  INTEGER NOT NULL,
      ctx_hash   INTEGER NOT NULL,
      prompt     TEXT    NOT NULL,
      model      TEXT    NOT NULL,
      via        TEXT    NOT NULL,
      variant    TEXT    NOT NULL,
      level      TEXT    NOT NULL,
      json       TEXT    NOT NULL,
      created_at INTEGER NOT NULL,
      PRIMARY KEY (norm_hash, ctx_hash, prompt, model, via, variant, level)
    ) WITHOUT ROWID;
    INSERT INTO summary
      SELECT norm_hash, ctx_hash, prompt, model, 'api', variant, level, json, created_at
        FROM summary_v1;
    DROP TABLE summary_v1;
    "#,
)];
