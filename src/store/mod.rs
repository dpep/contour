//! SQLite, WAL, and nothing clever.
//!
//! Units are keyed by blob OID, so two worktrees of one repo store one copy
//! and a branch switch reparses only what is genuinely new (DEC-003). The
//! store's job is to make that diff cheap and to stay out of the way
//! otherwise.
//!
//! Conventions (pragmas, `user_version` as the version marker, `$CONTOUR_DB`)
//! follow trekr's `src/store/`, which follows rq's.

mod schema;

use crate::core::{Blob, Oid, Param, ParamKind, Unit};
use crate::scan::Files;
use crate::summary::Summary;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Store {
    conn: Connection,
}

/// What one indexing pass did. Every count is honest about *work*, not about
/// contents: `parsed` is the only expensive number in it.
#[derive(Debug, Default, serde::Serialize)]
pub struct Indexed {
    pub files: usize,
    /// Distinct blobs the checkout references.
    pub blobs: usize,
    /// Blobs whose bytes this machine had never seen. A reindex with no edits
    /// makes this zero, which is the entire point of blob keying.
    pub parsed: usize,
    /// Units found in the blobs `parsed` above — so zero on a no-op reindex,
    /// and never the size of the checkout. `Checkout::units` is that number.
    pub units: usize,
}

/// One indexed checkout, as `--status` reports it.
#[derive(Debug, serde::Serialize)]
pub struct Checkout {
    pub root: String,
    pub indexed_at: i64,
    pub files: i64,
    pub blobs: i64,
    /// Callables reachable in this checkout, counted **once per path**. Two
    /// files with identical bytes share one blob and one set of stored rows,
    /// but they are two places a reader would find the method, so they count
    /// twice here. That makes this legitimately larger than the number of
    /// `unit` rows — on rails, 54,296 against 54,068 — and the gap is exactly
    /// the `files` − `blobs` difference doing its job.
    pub units: i64,
    /// git has touched its index since we last looked. A probe, not a proof —
    /// see `scan::git_fingerprint` for what it cannot see.
    pub stale: bool,
}

/// Where the database lives: `$CONTOUR_DB`, else
/// `~/.local/share/contour/contour.db` (DEC-009).
pub fn default_path() -> Result<PathBuf> {
    Ok(match std::env::var("CONTOUR_DB") {
        Ok(path) => PathBuf::from(path),
        Err(_) => PathBuf::from(std::env::var("HOME")?).join(".local/share/contour/contour.db"),
    })
}

/// The database every command uses.
pub fn open_default() -> Result<Store> {
    let path = default_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Store::open(&path)
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Store::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        // WAL lets a reader answer while an indexer writes; busy_timeout makes
        // a second writer wait rather than fail.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; \
             PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-32768;",
        )?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        // An *older* binary must not drop a newer database. Two contours on
        // one machine — one installed, one freshly built — would otherwise
        // take turns wiping each other's index, and each would look like it
        // had simply never been run.
        anyhow::ensure!(
            version <= schema::VERSION,
            "database is schema v{version} but this contour speaks v{}; \
             upgrade contour, or point $CONTOUR_DB elsewhere",
            schema::VERSION
        );
        if version != schema::VERSION {
            // No migration, by design: see schema::VERSION.
            if version != 0 {
                conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
                for table in schema::TABLES {
                    conn.execute_batch(&format!("DROP TABLE IF EXISTS {table};"))?;
                }
                conn.execute_batch("PRAGMA foreign_keys=ON;")?;
            }
            conn.execute_batch(schema::SCHEMA)?;
            conn.pragma_update(None, "user_version", schema::VERSION)?;
        }

        // The purchased layer, applied on every open and dropped by nothing
        // above. Its version lives in a row rather than in `user_version`
        // precisely so the rebuild cannot touch it.
        conn.execute_batch(schema::SUMMARY_SCHEMA)?;
        let stored: Option<i64> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'summary_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|v| v.parse().ok());
        match stored {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('summary_version', ?1)",
                    params![schema::SUMMARY_VERSION.to_string()],
                )?;
            }
            // Refuse rather than guess. Summaries cannot be recomputed from
            // local bytes, so a mismatch is a migration someone has to write.
            Some(found) => anyhow::ensure!(
                found == schema::SUMMARY_VERSION,
                "the summary table is v{found} but this contour speaks \
                 v{}; summaries cannot be regenerated for free, so they are \
                 not dropped automatically",
                schema::SUMMARY_VERSION
            ),
        }
        Ok(Store { conn })
    }

    /// Blob OIDs this machine has already read, of the ones asked about.
    ///
    /// Loaded whole rather than probed per OID: at 100k blobs it is a few MB
    /// and one query, where the probe is 100k round trips.
    pub fn known(&self, wanted: &HashSet<Oid>) -> Result<HashSet<Oid>> {
        let mut stmt = self.conn.prepare("SELECT oid FROM blob")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut known = HashSet::new();
        for oid in rows {
            let oid = Oid(oid?);
            if wanted.contains(&oid) {
                known.insert(oid);
            }
        }
        Ok(known)
    }

    /// Record one checkout's file map and any blobs it brought with it.
    ///
    /// One transaction: an interrupted index leaves the previous state intact
    /// rather than a half-mapped checkout.
    pub fn write(
        &mut self,
        root: &str,
        files: &Files,
        parsed: Vec<(Oid, Blob)>,
        git_state: i64,
    ) -> Result<Indexed> {
        let tx = self.conn.transaction()?;
        let mut counts = Indexed {
            files: files.len(),
            parsed: parsed.len(),
            ..Indexed::default()
        };

        for (oid, blob) in &parsed {
            counts.units += blob.units.len();
            insert_blob(&tx, oid, blob)?;
        }

        tx.execute(
            "INSERT OR IGNORE INTO checkout (root, indexed_at, map_key, git_state)
             VALUES (?1, unixepoch(), 0, 0)",
            params![root],
        )?;
        let checkout_id: i64 = tx.query_row(
            "SELECT id FROM checkout WHERE root = ?1",
            params![root],
            |r| r.get(0),
        )?;

        // What the map *would* be, folded before any of it is written. When it
        // matches what is stored the map is identical and the rewrite below is
        // pure cost — which on a no-op index is the only cost left, and the
        // one that grows with the repo.
        let map_key = files.iter().fold(0i64, |key, (path, oid)| {
            key.wrapping_add(path_hash(path) ^ path_hash(&oid.0))
        });
        // `EXISTS` rather than `COUNT`: the question is whether the map was
        // ever written, and counting it would put an O(files) scan back into
        // the path this exists to make O(1). A stored key of 0 against a map
        // with no rows is the initial state, not a match.
        let (stored_key, mapped): (i64, bool) = tx.query_row(
            "SELECT map_key, EXISTS(SELECT 1 FROM file WHERE checkout_id = ?1)
               FROM checkout WHERE id = ?1",
            params![checkout_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        if stored_key == map_key && mapped {
            counts.blobs = files.values().collect::<HashSet<&Oid>>().len();
            // Still record git's view: the map did not move, but git's index
            // may have — a commit touching no Ruby file, say — and leaving the
            // old fingerprint would make `--status` cry stale forever.
            tx.execute(
                "UPDATE checkout SET indexed_at = unixepoch(), git_state = ?2 WHERE id = ?1",
                params![checkout_id, git_state],
            )?;
            tx.commit()?;
            return Ok(counts);
        }

        // The map is rewritten wholesale. It is one row per file, and a delta
        // would have to be right about deletes and renames to save a few
        // milliseconds.
        tx.execute(
            "DELETE FROM file WHERE checkout_id = ?1",
            params![checkout_id],
        )?;
        {
            let mut ids: HashMap<&Oid, i64> = HashMap::new();
            let mut lookup = tx.prepare("SELECT id FROM blob WHERE oid = ?1")?;
            let mut insert =
                tx.prepare("INSERT INTO file (checkout_id, path, blob_id) VALUES (?1, ?2, ?3)")?;
            for (path, oid) in files {
                let id = match ids.get(oid) {
                    Some(found) => *found,
                    None => {
                        let found: i64 = lookup.query_row(params![oid.0], |r| r.get(0))?;
                        ids.insert(oid, found);
                        found
                    }
                };
                insert.execute(params![checkout_id, path, id])?;
            }
            counts.blobs = ids.len();
        }

        tx.execute(
            "UPDATE checkout SET indexed_at = unixepoch(), map_key = ?2, git_state = ?3
              WHERE id = ?1",
            params![checkout_id, map_key, git_state],
        )?;
        tx.commit()?;
        Ok(counts)
    }

    /// One row per indexed checkout.
    pub fn status(&self) -> Result<Vec<Checkout>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.root, c.indexed_at, c.git_state,
                    COUNT(f.path), COUNT(DISTINCT f.blob_id),
                    COALESCE(SUM((SELECT COUNT(*) FROM unit u WHERE u.blob_id = f.blob_id)), 0)
               FROM checkout c LEFT JOIN file f ON f.checkout_id = c.id
              GROUP BY c.id ORDER BY c.root",
        )?;
        let rows = stmt.query_map([], |r| {
            let root: String = r.get(0)?;
            let git_state: i64 = r.get(2)?;
            let stale =
                crate::scan::git_fingerprint(Path::new(&root)).is_some_and(|now| now != git_state);
            Ok(Checkout {
                root,
                indexed_at: r.get(1)?,
                files: r.get(3)?,
                blobs: r.get(4)?,
                units: r.get(5)?,
                stale,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every unit in a checkout, with the path it currently lives at.
    ///
    /// The path comes from the file map rather than the unit, which is the
    /// layer-1 contract paying off: one blob's units are stored once however
    /// many paths point at it.
    pub fn units(&self, root: &str) -> Result<Vec<Located>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, u.name, u.owner, u.singleton, u.params, u.via, u.line, u.end_line,
                    u.norm_hash
               FROM checkout c
               JOIN file f ON f.checkout_id = c.id
               JOIN unit u ON u.blob_id = f.blob_id
              WHERE c.root = ?1
              ORDER BY f.path, u.line",
        )?;
        let rows = stmt.query_map(params![root], |r| {
            Ok(Located {
                path: r.get(0)?,
                unit: Unit {
                    name: r.get(1)?,
                    owner: r.get(2)?,
                    singleton: r.get::<_, i64>(3)? != 0,
                    params: decode_params(&r.get::<_, String>(4)?),
                    via: r.get(5)?,
                    line: r.get(6)?,
                    end_line: r.get(7)?,
                    norm_hash: r.get::<_, Option<i64>>(8)?.map(|h| h as u64),
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The summary stored under this exact key, if there is one.
    pub fn summary(&self, key: &SummaryKey<'_>) -> Result<Option<Summary>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM summary
                  WHERE norm_hash = ?1 AND ctx_hash = ?2 AND prompt = ?3
                    AND model = ?4 AND variant = 'body' AND level = 'unit'",
                params![
                    key.norm_hash as i64,
                    key.ctx_hash as i64,
                    key.prompt,
                    key.model
                ],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match json {
            Some(json) => Some(serde_json::from_str(&json)?),
            None => None,
        })
    }

    /// Record one purchased answer. Idempotent: re-running a fill that was
    /// interrupted must not fail on what it already bought.
    pub fn put_summary(&mut self, key: &SummaryKey<'_>, summary: &Summary) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO summary
               (norm_hash, ctx_hash, prompt, model, variant, level, json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'body', 'unit', ?5, unixepoch())",
            params![
                key.norm_hash as i64,
                key.ctx_hash as i64,
                key.prompt,
                key.model,
                serde_json::to_string(summary)?
            ],
        )?;
        Ok(())
    }

    /// Which summary keys, of the ones asked about, are already bought.
    ///
    /// Loaded in one query rather than probed per unit, for the same reason
    /// `known` is: at 50k units the probe is 50k round trips.
    pub fn have_summaries(&self, prompt: &str, model: &str) -> Result<HashSet<(u64, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT norm_hash, ctx_hash FROM summary
              WHERE prompt = ?1 AND model = ?2 AND variant = 'body' AND level = 'unit'",
        )?;
        let rows = stmt.query_map(params![prompt, model], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
    }

    /// Which models this machine has bought summaries from. `--status` reports
    /// coverage per model rather than for a presumed one: DEC-005 lets indexes
    /// from different models coexist, so "how covered am I" has no single
    /// answer.
    pub fn summary_models(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT model FROM summary ORDER BY model")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
}

/// Everything that decides which stored answer a unit gets. Borrowed rather
/// than owned: `prompt` and `model` are the same two strings for every unit in
/// a run, and cloning them per lookup is pure noise.
pub struct SummaryKey<'a> {
    pub norm_hash: u64,
    pub ctx_hash: u64,
    pub prompt: &'a str,
    pub model: &'a str,
}

/// A unit plus where this checkout currently keeps it.
#[derive(Debug, serde::Serialize)]
pub struct Located {
    pub path: String,
    #[serde(flatten)]
    pub unit: Unit,
}

fn insert_blob(tx: &rusqlite::Transaction<'_>, oid: &Oid, blob: &Blob) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO blob (oid, lines, parse_errors) VALUES (?1, ?2, ?3)",
        params![oid.0, blob.lines as i64, blob.parse_errors as i64],
    )?;
    let blob_id: i64 = tx.query_row("SELECT id FROM blob WHERE oid = ?1", params![oid.0], |r| {
        r.get(0)
    })?;
    // A blob already read is a blob already correct — same bytes, same units.
    // Re-inserting would duplicate every row.
    let already: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM unit WHERE blob_id = ?1)",
        params![blob_id],
        |r| r.get(0),
    )?;
    if already {
        return Ok(());
    }
    let mut stmt = tx.prepare_cached(
        "INSERT INTO unit (blob_id, name, owner, singleton, params, via, line, end_line, norm_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for u in &blob.units {
        stmt.execute(params![
            blob_id,
            u.name,
            u.owner,
            u.singleton as i64,
            encode_params(&u.params),
            u.via,
            u.line,
            u.end_line,
            u.norm_hash.map(|h| h as i64),
        ])?;
    }
    Ok(())
}

/// FNV-1a over a path, for the map fold. Not a stored key, so stability across
/// releases is not required — but it is the same ten lines as `crate::hash`
/// and reusing them costs nothing.
fn path_hash(s: &str) -> i64 {
    crate::hash::fnv1a(crate::hash::FNV_OFFSET, s.as_bytes()) as i64
}

/// Parameters round-trip through one column as `kind:name` pairs joined by
/// `;`, using Ruby's own `Method#parameters` vocabulary so the encoding needs
/// no glossary of ours.
fn encode_params(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| format!("{}:{}", p.kind.as_str(), p.name))
        .collect::<Vec<_>>()
        .join(";")
}

fn decode_params(s: &str) -> Vec<Param> {
    s.split(';')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (kind, name) = part.split_once(':')?;
            Some(Param {
                kind: ParamKind::parse(kind)?,
                name: name.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(src: &str) -> (Oid, Blob) {
        (
            crate::scan::hash_blob(src.as_bytes()),
            crate::ruby::units(src.as_bytes()),
        )
    }

    #[test]
    fn params_round_trip_through_one_column() {
        for params in [
            vec![],
            vec![Param {
                kind: ParamKind::Keyreq,
                name: "force".into(),
            }],
        ] {
            assert_eq!(decode_params(&encode_params(&params)), params);
        }
    }

    /// The whole premise of blob keying: a second checkout of the same bytes
    /// parses nothing and stores nothing new.
    #[test]
    fn a_second_checkout_of_the_same_blob_costs_nothing() {
        let mut store = Store::open_in_memory().unwrap();
        let (oid, parsed) = blob("class Widget\n  def save; end\nend\n");
        let files: Files = [("a.rb".to_string(), oid.clone())].into_iter().collect();

        store
            .write("/one", &files, vec![(oid.clone(), parsed)], 0)
            .unwrap();
        let known = store.known(&[oid.clone()].into_iter().collect()).unwrap();
        assert_eq!(known.len(), 1, "the blob is now known");

        // A second worktree offers the same OID, so nothing is re-parsed.
        let second = store.write("/two", &files, vec![], 0).unwrap();
        assert_eq!(second.parsed, 0);
        assert_eq!(store.units("/two").unwrap().len(), 1);
        assert_eq!(store.units("/one").unwrap()[0].unit.id(), "Widget#save");
    }

    /// A no-op reindex must not double the unit rows, and must not rewrite the
    /// file map.
    #[test]
    fn reindexing_an_unchanged_checkout_is_idempotent() {
        let mut store = Store::open_in_memory().unwrap();
        let (oid, parsed) = blob("class Widget\n  def save; end\n  def load; end\nend\n");
        let files: Files = [("a.rb".to_string(), oid.clone())].into_iter().collect();

        store
            .write("/r", &files, vec![(oid.clone(), parsed)], 0)
            .unwrap();
        store.write("/r", &files, vec![], 7).unwrap();
        assert_eq!(store.units("/r").unwrap().len(), 2);
        assert_eq!(store.status().unwrap()[0].units, 2);
    }

    /// Two paths pointing at one blob: units are stored once and located
    /// twice. This is the layer-1/layer-2 split doing its job.
    #[test]
    fn one_blob_at_two_paths_is_parsed_once_and_found_twice() {
        let mut store = Store::open_in_memory().unwrap();
        let (oid, parsed) = blob("def helper; end\n");
        let files: Files = [("a.rb".into(), oid.clone()), ("b/a.rb".into(), oid.clone())]
            .into_iter()
            .collect();

        let counts = store.write("/r", &files, vec![(oid, parsed)], 0).unwrap();
        assert_eq!((counts.files, counts.blobs, counts.parsed), (2, 1, 1));
        let located = store.units("/r").unwrap();
        assert_eq!(located.len(), 2);
        assert_eq!(located[0].path, "a.rb");
        assert_eq!(located[1].path, "b/a.rb");

        // The two `units` counts legitimately disagree, and the difference is
        // the point rather than a bug: one is work done, the other is what the
        // checkout contains. Pinned so nobody "fixes" one to match the other.
        assert_eq!(counts.units, 1, "one blob was parsed, holding one unit");
        assert_eq!(store.status().unwrap()[0].units, 2, "found at two paths");
    }
}
