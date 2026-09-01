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

use crate::core::{Blob, Lang, Oid, Param, ParamKind, Unit};
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
    /// The checkout's file map was not what the index held, so it was
    /// rewritten. False on a no-op reindex, and the fact a query path needs:
    /// `parsed` cannot answer it, because a deleted or renamed file moves the
    /// map without giving anything new to read.
    pub changed: bool,
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
    /// The checkout's current bytes no longer match what was indexed.
    ///
    /// Exact rather than a probe: the path→blob map is recomputed and compared
    /// to the one the index was built from, so a working-tree edit counts —
    /// which is the state a live session is always in, and the one the old
    /// `.git/index` stat could not see.
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

/// The checkouts a status question is about: every one, or just the one
/// containing `path`.
///
/// Shared by `--status` and the MCP `status` tool so "which checkout does this
/// path mean" has one answer rather than two that almost agree.
///
/// Deliberately not an index refresh (DEC-015): status is a flag, so it reports
/// on the database rather than filling it, and a path nothing has indexed is an
/// empty answer rather than a scan.
pub fn checkouts(store: &Store, path: Option<&Path>) -> Result<Vec<Checkout>> {
    let all = store.status()?;
    let Some(path) = path else {
        return Ok(all);
    };
    let root = crate::scan::repo_root(path)?;
    let root = root.to_string_lossy();
    Ok(all.into_iter().filter(|c| c.root == root).collect())
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
    /// Open the store `path` names.
    ///
    /// `path` is the **purchased** database — the one `$CONTOUR_DB` configures,
    /// `--status` prints, and nothing may ever drop. The derived half lives
    /// beside it in a file whose name carries the schema version
    /// ([`schema::derived_file`]), and is opened as the main database with the
    /// purchased one attached: a contour that speaks a different derived version
    /// builds its own file and neither can wipe or be refused by the other's.
    pub fn open(path: &Path) -> Result<Store> {
        let derived = schema::derived_file(path);
        let conn = Connection::open(&derived)?;
        // SQLite opens lazily, so the first statement is what discovers a file
        // that is not a database at all. Probing here keeps that failure apart
        // from the one `init` raises for a purchased schema it does not know —
        // which must never be answered with "delete it and reindex", because
        // deleting it is exactly what DEC-016 refuses to do.
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .map_err(|err| {
                anyhow::anyhow!(
                    "{} is not a readable database ({err}). It holds only derived \
                     data, so if it is corrupt, delete it — everything in it \
                     rebuilds from local bytes in seconds.",
                    derived.display()
                )
            })?;
        Store::init(conn, &path.to_string_lossy())
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?, ":memory:")
    }

    fn init(conn: Connection, purchased: &str) -> Result<Store> {
        // WAL lets a reader answer while an indexer writes; busy_timeout makes
        // a second writer wait rather than fail.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; \
             PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-32768;",
        )?;
        // The derived half is whatever this file holds, and the file is named
        // for the version — so a mismatch is not a state that can arise, and
        // there is nothing here to drop or migrate. An empty file is a new one.
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != schema::VERSION {
            conn.execute_batch(schema::SCHEMA)?;
            conn.pragma_update(None, "user_version", schema::VERSION)?;
        }

        // The purchased half, in its own file: one store, unversioned in its
        // name, that every contour version reaches through the migration
        // discipline below (DEC-016). Splitting it per version would orphan paid
        // work on every upgrade, which is the one thing this must never do.
        conn.execute("ATTACH DATABASE ?1 AS purchased", params![purchased])?;
        conn.execute_batch(schema::SUMMARY_SCHEMA)?;
        let stored: Option<i64> = conn
            .query_row(
                "SELECT value FROM purchased.meta WHERE key = 'summary_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|v| v.parse().ok());
        match stored {
            None => {
                conn.execute(
                    "INSERT INTO purchased.meta (key, value) VALUES ('summary_version', ?1)",
                    params![schema::SUMMARY_VERSION.to_string()],
                )?;
            }
            // A *newer* purchased schema. The derived half has guarded this
            // since milestone 1; this half did not, and the gap was worse than
            // an omission: the migration walk below runs only upward, so a
            // marker of 99 fell straight through it to the `UPDATE` and got
            // stamped back down to ours — silently rewriting the one record
            // that says what format the summaries are in. DEC-016 says the
            // refusal is the point, and this is where it has to happen.
            Some(found) if found > schema::SUMMARY_VERSION => {
                anyhow::bail!(
                    "the summary table is v{found} but this contour speaks v{}; \
                     a newer contour wrote it. Summaries are purchased work, so \
                     this build will not guess at a format it does not know — \
                     upgrade contour, or point $CONTOUR_DB elsewhere.",
                    schema::SUMMARY_VERSION
                );
            }
            Some(found) if found != schema::SUMMARY_VERSION => {
                // Migrate rather than drop. Summaries cannot be recomputed
                // from local bytes, so the tripwire only fires when no
                // migration covers the gap — and then it refuses to open
                // rather than guessing (DEC-016).
                let mut at = found;
                while at < schema::SUMMARY_VERSION {
                    let step = schema::SUMMARY_MIGRATIONS
                        .iter()
                        .find(|(from, _)| *from == at);
                    let Some((_, sql)) = step else {
                        anyhow::bail!(
                            "the summary table is v{at} but this contour speaks v{}, \
                             and no migration covers the gap. Summaries cannot be \
                             regenerated for free, so they are not dropped \
                             automatically — move $CONTOUR_DB aside to start fresh.",
                            schema::SUMMARY_VERSION
                        );
                    };
                    conn.execute_batch(sql)?;
                    at += 1;
                }
                conn.execute(
                    "UPDATE purchased.meta SET value = ?1 WHERE key = 'summary_version'",
                    params![schema::SUMMARY_VERSION.to_string()],
                )?;
            }
            Some(_) => {}
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
            "INSERT OR IGNORE INTO checkout (root, indexed_at, map_key)
             VALUES (?1, unixepoch(), 0)",
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
        // one that grows with the repo. The same fold answers `stale`.
        let map_key = crate::scan::map_key(files);
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
            tx.execute(
                "UPDATE checkout SET indexed_at = unixepoch() WHERE id = ?1",
                params![checkout_id],
            )?;
            tx.commit()?;
            return Ok(counts);
        }

        // The map is rewritten wholesale. It is one row per file, and a delta
        // would have to be right about deletes and renames to save a few
        // milliseconds.
        counts.changed = true;
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
            "UPDATE checkout SET indexed_at = unixepoch(), map_key = ?2 WHERE id = ?1",
            params![checkout_id, map_key],
        )?;
        tx.commit()?;
        Ok(counts)
    }

    /// One row per indexed checkout.
    pub fn status(&self) -> Result<Vec<Checkout>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.root, c.indexed_at, c.map_key,
                    COUNT(f.path), COUNT(DISTINCT f.blob_id),
                    COALESCE(SUM((SELECT COUNT(*) FROM unit u WHERE u.blob_id = f.blob_id)), 0)
               FROM checkout c LEFT JOIN file f ON f.checkout_id = c.id
              GROUP BY c.id ORDER BY c.root",
        )?;
        let rows = stmt.query_map([], |r| {
            let root: String = r.get(0)?;
            let indexed_map: i64 = r.get(2)?;
            // Rescanning is ~20 ms and answers exactly; a stat of `.git/index`
            // was instant and answered wrongly for every uncommitted edit.
            // An unreadable checkout is not stale, it is gone — and `files`
            // being empty is a real answer about a repo with no source in it.
            let stale = crate::scan::scan(Path::new(&root))
                .is_ok_and(|files| crate::scan::map_key(&files) != indexed_map);
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
            "SELECT f.path, u.lang, u.name, u.owner, u.singleton, u.params, u.via,
                    u.line, u.end_line, u.norm_hash, u.nodes, u.visibility
               FROM checkout c
               JOIN file f ON f.checkout_id = c.id
               JOIN unit u ON u.blob_id = f.blob_id
              WHERE c.root = ?1
              ORDER BY f.path, u.line",
        )?;
        let rows = stmt.query_map(params![root], |r| {
            let path: String = r.get(0)?;
            let mut unit = Unit {
                // An unknown language means the row was written by a
                // newer contour; treat it as Ruby rather than dropping a
                // unit, since every field but rendering is neutral.
                lang: Lang::parse(&r.get::<_, String>(1)?).unwrap_or(Lang::Ruby),
                name: r.get(2)?,
                owner: r.get(3)?,
                singleton: r.get::<_, i64>(4)? != 0,
                params: decode_params(&r.get::<_, String>(5)?),
                via: r.get(6)?,
                line: r.get(7)?,
                end_line: r.get(8)?,
                norm_hash: r.get::<_, Option<i64>>(9)?.map(|h| h as u64),
                nodes: r.get::<_, Option<i64>>(10)?.map(|n| n as u32),
                // An unrecognized word means a newer contour wrote the row.
                // Public is the reading that hides nothing, which is the safe
                // direction for a field whose only consumer nominates.
                visibility: crate::core::Visibility::parse(&r.get::<_, String>(11)?)
                    .unwrap_or_default(),
            };
            // The file layer, doing the one job DEC-021 reserves for it: the
            // row holds the bare lexical owner, and the path says which module
            // it is in. Written back is not an option — that would make layer 1
            // depend on where a blob happens to sit.
            crate::paths::qualify(&path, &mut unit);
            Ok(Located { path, unit })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Record one purchased answer. Idempotent: re-running a fill that was
    /// interrupted must not fail on what it already bought.
    ///
    /// The one door into the purchased half, and therefore where
    /// [`Summary::check`] is enforced — a gate on any lesser path is a gate
    /// the next path forgets.
    pub fn put_summary(&mut self, key: &SummaryKey<'_>, summary: &Summary) -> Result<()> {
        summary.check()?;
        self.conn.execute(
            "INSERT OR REPLACE INTO purchased.summary
               (norm_hash, ctx_hash, prompt, model, via, variant, level, json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'body', 'unit', ?6, unixepoch())",
            params![
                key.norm_hash as i64,
                key.ctx_hash as i64,
                key.prompt,
                key.model,
                key.via,
                serde_json::to_string(summary)?
            ],
        )?;
        Ok(())
    }

    /// Which summary keys, of the ones asked about, are already bought.
    ///
    /// Scoped to one `via` on purpose: a uniform API fill must not count a
    /// session's contribution as already done, or the set it produces stops
    /// being uniform — which is exactly the property the Phase 1 calibration
    /// needs (DEC-018).
    ///
    /// Loaded in one query rather than probed per unit, for the same reason
    /// `known` is: at 50k units the probe is 50k round trips.
    pub fn have_summaries(
        &self,
        prompt: &str,
        model: &str,
        via: &str,
    ) -> Result<HashSet<(u64, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT norm_hash, ctx_hash FROM purchased.summary
              WHERE prompt = ?1 AND model = ?2 AND via = ?3
                AND variant = 'body' AND level = 'unit'",
        )?;
        let rows = stmt.query_map(params![prompt, model, via], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
    }

    /// The vectors for one embedder configuration and a given set of texts,
    /// keyed by the text each came from.
    ///
    /// **`wanted` is the scope, expressed in the only vocabulary this table
    /// has.** A vector is keyed by its text, not by a unit or a path
    /// (DEC-003), so the caller resolves its scope to the texts those units
    /// embed as and asks for exactly those. Reading the whole table instead
    /// was a floor no scope could lower — measured at ~1 s and 750 MB of
    /// resident memory for 132k units, and linear from there (docs/PLAN.md).
    ///
    /// Two ways to answer, because neither wins everywhere: reading the table
    /// through costs what the **table** holds, and looking each key up costs
    /// what the **caller asked for**. Measured on a 117,556-vector table, a
    /// read-through is ~1.3 µs a row and a lookup ~4.2 µs a key, so the
    /// read-through wins until the table is about three times the size of the
    /// request.
    ///
    /// Which side of that a call falls on is not something the caller can
    /// know: this table holds every checkout on the machine, so even "the
    /// whole repository" may be a small slice of it. So the read-through is
    /// tried and **abandoned** once it has visited more rows than a lookup per
    /// key would have cost — self-correcting, and needing no statistics, which
    /// is just as well because `count(*)` on this table costs as much as
    /// reading it.
    pub fn vectors(
        &self,
        config_key: u64,
        wanted: &std::collections::HashSet<u64>,
    ) -> Result<HashMap<u64, Vec<f32>>> {
        /// Rows a read-through may visit per key asked for, before looking
        /// them up one at a time is the cheaper answer. From the two rates
        /// above: 4.2 / 1.3.
        const SCAN_RATIO: usize = 3;

        if wanted.is_empty() {
            return Ok(HashMap::new());
        }
        let budget = wanted.len().saturating_mul(SCAN_RATIO);
        let mut out = HashMap::with_capacity(wanted.len());
        let mut stmt = self
            .conn
            .prepare_cached("SELECT text_key, vec FROM vector WHERE config_key = ?1")?;
        let mut rows = stmt.query(params![config_key as i64])?;
        let mut visited = 0usize;
        while let Some(row) = rows.next()? {
            visited += 1;
            if visited > budget {
                drop(rows);
                return self.vectors_by_key(config_key, wanted);
            }
            let key = row.get::<_, i64>(0)? as u64;
            if wanted.contains(&key) {
                out.insert(key, decode_vector(&row.get::<_, Vec<u8>>(1)?));
            }
        }
        Ok(out)
    }

    /// One lookup per key. The other half of [`Store::vectors`]; never called
    /// directly, because which half is right is a property of the table rather
    /// than of the caller.
    fn vectors_by_key(
        &self,
        config_key: u64,
        wanted: &std::collections::HashSet<u64>,
    ) -> Result<HashMap<u64, Vec<f32>>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT vec FROM vector WHERE config_key = ?1 AND text_key = ?2")?;
        let mut out = HashMap::with_capacity(wanted.len());
        // Ascending, so the seeks walk the b-tree forwards instead of jumping
        // about in it — measured 20% faster on a whole-checkout request.
        let mut keys: Vec<u64> = wanted.iter().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let found = stmt
                .query_row(params![config_key as i64, key as i64], |r| {
                    r.get::<_, Vec<u8>>(0)
                })
                .optional()?;
            if let Some(bytes) = found {
                out.insert(key, decode_vector(&bytes));
            }
        }
        Ok(out)
    }

    /// Store freshly embedded vectors. One transaction, so an interrupted
    /// embed leaves a consistent set rather than a torn one.
    pub fn put_vectors(&mut self, config_key: u64, vectors: &[(u64, Vec<f32>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO vector (config_key, text_key, vec) VALUES (?1, ?2, ?3)",
            )?;
            for (text_key, vec) in vectors {
                stmt.execute(params![
                    config_key as i64,
                    *text_key as i64,
                    encode_vector(vec)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every stored summary, keyed by `(norm_hash, ctx_hash)`.
    ///
    /// Across models on purpose: DEC-005 lets indexes from different models
    /// coexist, and refusing to answer from a summary because it came from a
    /// model other than today's default would hide work already paid for. When
    /// several models have summarized the same body, the most recent wins.
    ///
    /// One query rather than a probe per unit — at 50k units the probe is 50k
    /// round trips, which is the same mistake `known` exists to avoid.
    pub fn all_summaries(&self) -> Result<HashMap<(u64, u64), Summary>> {
        let mut stmt = self.conn.prepare(
            "SELECT norm_hash, ctx_hash, json FROM purchased.summary
              WHERE variant = 'body' AND level = 'unit'
              ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (norm_hash, ctx_hash, json) = row?;
            out.insert((norm_hash, ctx_hash), serde_json::from_str(&json)?);
        }
        Ok(out)
    }

    /// Every stored signature, as `norm_hash -> its sub-shapes`.
    ///
    /// Loaded whole, like `vectors`: the near tier's inverted index needs all
    /// of it, and one query beats a probe per body.
    pub fn signatures(&self) -> Result<HashMap<u64, Vec<crate::core::Subtree>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT norm_hash, subtree_hash, nodes, parent_hash FROM signature")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                crate::core::Subtree {
                    hash: r.get::<_, i64>(1)? as u64,
                    nodes: r.get(2)?,
                    parent: r.get::<_, i64>(3)? as u64,
                },
            ))
        })?;
        let mut out: HashMap<u64, Vec<crate::core::Subtree>> = HashMap::new();
        for row in rows {
            let (norm_hash, subtree) = row?;
            out.entry(norm_hash).or_default().push(subtree);
        }
        Ok(out)
    }

    /// Every `(model, via)` this machine has summaries from.
    ///
    /// Both halves of the key, not just the model: DEC-005 lets indexes from
    /// different models coexist and DEC-018 keeps a session's contributions in
    /// their own keyspace, so "how covered am I" has no single answer — and
    /// reporting per model alone made `--status` blind to every contribution,
    /// which is how it came to say `none 0/128` about a corpus `search` was
    /// already answering from.
    pub fn summary_sources(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT model, via FROM purchased.summary ORDER BY model, via")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
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
    /// Who bought the answer: [`VIA_API`] or [`VIA_MCP`].
    pub via: &'a str,
}

/// contour called a model itself (or replayed a fixture through the same path).
pub const VIA_API: &str = "api";
/// A session handed contour a summary it wrote.
///
/// The value says `mcp` because that was the only door when it was chosen, and
/// it is a **key** column: renaming it to match the CLI door that arrived later
/// would re-key every contribution, which DEC-016 says costs the work again.
/// What it distinguishes is who paid — a session's attention against an API
/// fill — and that is the same on both doors, which is why one value is right.
pub const VIA_MCP: &str = "mcp";

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
    {
        // Idempotent by primary key: the same body reached from a second blob
        // writes the same rows, and a clone writes them once.
        let mut sig = tx.prepare_cached(
            "INSERT OR IGNORE INTO signature (norm_hash, subtree_hash, nodes, parent_hash)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (norm_hash, subtrees) in &blob.signatures {
            for subtree in subtrees {
                sig.execute(params![
                    *norm_hash as i64,
                    subtree.hash as i64,
                    subtree.nodes,
                    subtree.parent as i64
                ])?;
            }
        }
    }
    let mut stmt = tx.prepare_cached(
        "INSERT INTO unit
           (blob_id, lang, name, owner, singleton, params, via, line, end_line,
            norm_hash, nodes, visibility)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    for u in &blob.units {
        stmt.execute(params![
            blob_id,
            u.lang.as_str(),
            u.name,
            u.owner,
            u.singleton as i64,
            encode_params(&u.params),
            u.via,
            u.line,
            u.end_line,
            u.norm_hash.map(|h| h as i64),
            u.nodes.map(|n| n as i64),
            u.visibility.as_str(),
        ])?;
    }
    Ok(())
}

/// Vectors round-trip as little-endian f32. Explicit rather than
/// `bytemuck`-style casting: the byte order is an on-disk format and must not
/// follow whatever the host happens to be.
fn encode_vector(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes)
        .collect()
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

    /// DEC-016's refusal, which was not enforced: a marker newer than ours
    /// fell through the upward-only migration walk and was stamped back down,
    /// rewriting the record that says what format the purchased half is in.
    /// Found by QA hand-editing `meta`.
    #[test]
    fn a_newer_purchased_schema_refuses_to_open() {
        let path = std::env::temp_dir().join(format!("contour-dec016-{}.db", std::process::id()));
        let derived = schema::derived_file(&path);
        for file in [&path, &derived] {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", file.display()));
            }
        }
        Store::open(&path).expect("a fresh store opens");

        // Opened directly, so this file is `main` here and the purchased tables
        // are unqualified — the split is a fact about how `Store` attaches
        // them, not about the file.
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE meta SET value = '99' WHERE key = 'summary_version'",
            [],
        )
        .unwrap();
        drop(conn);

        let refused = Store::open(&path);
        assert!(refused.is_err(), "a newer purchased schema must refuse");
        let message = format!("{:#}", refused.err().unwrap());
        assert!(message.contains("v99"), "{message}");

        // And the marker is left exactly as it was found. Repairing it would
        // destroy the only evidence of what wrote the file.
        let conn = Connection::open(&path).unwrap();
        let still: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'summary_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still, "99");
        drop(conn);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&derived);
    }

    /// The split's whole purpose, and the constraint the owner set on it: the
    /// derived half is disposable — a version bump names a different file, which
    /// is the same thing as deleting this one — and the purchased half is not
    /// touched by that. Versioning the purchased half per schema instead would
    /// orphan paid work on every upgrade, which is the one outcome DEC-016
    /// exists to prevent.
    #[test]
    fn deleting_the_derived_half_keeps_what_was_purchased() {
        let path = std::env::temp_dir().join(format!("contour-split-{}.db", std::process::id()));
        let derived = schema::derived_file(&path);
        for file in [&path, &derived] {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", file.display()));
            }
        }

        let summary = Summary {
            summary: "Returns a customer's unpaid invoices.".into(),
            primary_purpose: "unpaid invoice lookup".into(),
            secondary_concerns: Vec::new(),
            side_effects: Vec::new(),
            domain: "billing".into(),
            patterns: Vec::new(),
        };
        let mut store = Store::open(&path).unwrap();
        store
            .put_summary(
                &SummaryKey {
                    norm_hash: 7,
                    ctx_hash: 11,
                    prompt: "v1",
                    model: "m",
                    via: VIA_API,
                },
                &summary,
            )
            .unwrap();
        drop(store);

        assert!(derived.exists(), "the derived half is its own file");
        std::fs::remove_file(&derived).unwrap();

        let store = Store::open(&path).unwrap();
        let kept = store.all_summaries().unwrap();
        assert_eq!(kept.len(), 1, "the purchased half survives");
        assert_eq!(kept.get(&(7, 11)), Some(&summary));
        // And the derived half is genuinely rebuilt rather than recovered.
        assert!(store.status().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(schema::derived_file(&path));
    }

    #[test]
    fn vectors_round_trip_through_a_blob() {
        let mut store = Store::open_in_memory().unwrap();
        let vec: Vec<f32> = vec![0.5, -0.25, 0.0, 1.0];
        store.put_vectors(7, &[(11, vec.clone())]).unwrap();
        assert_eq!(store.vectors(7, &keys(&[11])).unwrap().get(&11), Some(&vec));
        // A different embedder configuration is a different index, not an
        // overwrite (DEC-005).
        assert!(store.vectors(8, &keys(&[11])).unwrap().is_empty());
    }

    /// A read costs what the scope holds, not what the table holds — the
    /// warm floor a scope could not lower before (DEC-033). Asking for one of
    /// two stored vectors must return one: reading the tableful returns both,
    /// which is what this pins.
    ///
    /// Both halves run here, chosen by the sizes: one key out of 2,500 stored
    /// is answered key by key, and 2,000 of them by reading the table through.
    /// Neither is visible from outside, which is the point — the contract is
    /// "exactly what was asked for", and it has to hold whichever way the
    /// answer was reached.
    #[test]
    fn a_vector_read_is_bounded_by_what_was_asked_for() {
        let mut store = Store::open_in_memory().unwrap();
        let stored: Vec<(u64, Vec<f32>)> = (0..2_500u64).map(|k| (k, vec![k as f32])).collect();
        store.put_vectors(7, &stored).unwrap();

        let one = store.vectors(7, &keys(&[42])).unwrap();
        assert_eq!(one.len(), 1, "the scope was one vector");
        assert_eq!(one.get(&42), Some(&vec![42.0]));

        // Large enough to be read through, and asking for a key nothing
        // stored, which neither half may invent.
        let wide: Vec<u64> = (0..2_000).chain(std::iter::once(9_999)).collect();
        let many = store.vectors(7, &keys(&wide)).unwrap();
        assert_eq!(many.len(), 2_000, "every stored key, and only those");
        assert_eq!(many.get(&1_999), Some(&vec![1999.0]));

        assert!(store.vectors(7, &keys(&[])).unwrap().is_empty());
    }

    fn keys(of: &[u64]) -> std::collections::HashSet<u64> {
        of.iter().copied().collect()
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
            .write("/one", &files, vec![(oid.clone(), parsed)])
            .unwrap();
        let known = store.known(&[oid.clone()].into_iter().collect()).unwrap();
        assert_eq!(known.len(), 1, "the blob is now known");

        // A second worktree offers the same OID, so nothing is re-parsed.
        let second = store.write("/two", &files, vec![]).unwrap();
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
            .write("/r", &files, vec![(oid.clone(), parsed)])
            .unwrap();
        store.write("/r", &files, vec![]).unwrap();
        assert_eq!(store.units("/r").unwrap().len(), 2);
        assert_eq!(store.status().unwrap()[0].units, 2);
    }

    /// Visibility survives the round trip. It is a column rather than a
    /// derivation, so nothing re-reads the source to answer "may a caller call
    /// this" — and the nomination rule that asks (DEC-028) runs per query.
    #[test]
    fn who_may_call_a_unit_survives_the_store() {
        let mut store = Store::open_in_memory().unwrap();
        let (oid, parsed) = blob(
            "class W
  def call; end
  private
  def helper; end
end
",
        );
        let files: Files = [("a.rb".to_string(), oid.clone())].into_iter().collect();
        store.write("/r", &files, vec![(oid, parsed)]).unwrap();

        let seen: Vec<(String, &str)> = store
            .units("/r")
            .unwrap()
            .iter()
            .map(|l| (l.unit.id(), l.unit.visibility.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("W#call".to_string(), "public"),
                ("W#helper".to_string(), "private"),
            ]
        );
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

        let counts = store.write("/r", &files, vec![(oid, parsed)]).unwrap();
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
