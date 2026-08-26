//! The on-disk schema.

/// Bump on any change to the schema **or to what the extractor emits**.
///
/// The second half is easy to miss: units are cached by blob OID on the
/// premise that they are a pure function of the bytes — but when the
/// *function* changes, identical bytes must still be re-read. An extractor fix
/// otherwise ships silently dead, because every blob it would affect is
/// already "known".
///
/// There are no migrations, and that is deliberate (DEC-003): everything here
/// is derived from bytes this machine can read again, so the database is a
/// **cache of a pure function**, not a system of record. A version mismatch
/// drops it and reindexes — which costs seconds and removes an entire class of
/// migration bug. It also makes adding a column free, so nothing needs to be
/// carried speculatively.
pub(crate) const VERSION: i64 = 1;

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
  name      TEXT    NOT NULL,
  owner     TEXT    NOT NULL,             -- lexical namespace, '::'-joined
  singleton INTEGER NOT NULL,             -- `def self.x` / inside `class << self`
  params    TEXT    NOT NULL,             -- 'req:a;key:b', Ruby's vocabulary
  via       TEXT,
  line      INTEGER NOT NULL,
  end_line  INTEGER NOT NULL
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
  map_key    INTEGER NOT NULL,
  -- git's own view of the checkout when it was last indexed: one stat of
  -- `.git/index`, folded. Lets `--status` say "might be stale" in O(1)
  -- rather than rescanning to find out.
  git_state  INTEGER NOT NULL
);

CREATE TABLE file (
  checkout_id INTEGER NOT NULL REFERENCES checkout(id) ON DELETE CASCADE,
  path        TEXT    NOT NULL,           -- relative to the checkout root
  blob_id     INTEGER NOT NULL REFERENCES blob(id),
  PRIMARY KEY (checkout_id, path)
) WITHOUT ROWID;

CREATE INDEX unit_name ON unit(name);
CREATE INDEX unit_blob ON unit(blob_id);
CREATE INDEX file_blob ON file(blob_id);
"#;

/// Every table, dependents first, so a drop respects nothing.
pub(crate) const TABLES: [&str; 4] = ["file", "checkout", "unit", "blob"];
