//! Scan → parse → store, for one checkout.
//!
//! The only expensive step is parsing, and it runs on exactly the blobs this
//! machine has never seen. Everything else is a git call and a fold.

use crate::core::{Blob, Lang, Oid};
use crate::scan;
use crate::store::{Indexed, Store};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Index the checkout containing `path`. Returns the repo root and the work
/// that was actually done.
/// Bytes → units, for whichever extractor reads this path.
///
/// The one place that choice is made. It used to be made in four — `index`,
/// `--symbols`, the MCP `symbols` tool, and the fill loop's re-parse check —
/// and the fourth had it wrong, parsing every body as Ruby. That made
/// `summarize` reject every Rust unit in the corpus with "the file changed
/// since it was indexed", which is a false statement about the user's files
/// and unfixable by the reindex it asks for.
///
/// `None` when no extractor reads this path, which for anything the index
/// holds cannot happen — `scan::language` is what put it there.
pub fn units_at(path: &str, src: &[u8]) -> Option<Blob> {
    Some(match crate::scan::language(path)? {
        Lang::Ruby => crate::ruby::units(src),
        Lang::Rust => crate::rust::units(src),
    })
}

/// The store, ready to answer about the checkout containing `path`.
pub struct Opened {
    pub store: Store,
    pub root: String,
    /// What bringing the checkout up to date took. `changed` is false on the
    /// common path, and is what a surface discloses when it is not.
    pub refreshed: Indexed,
}

/// Open the index **and bring this checkout up to date**, before anything
/// answers from it.
///
/// The invariant: **a query answer is never silently stale.** `--status` could
/// already see a working-tree edit exactly (`scan::map_key`), but no query
/// consulted it — so `search` would happily return a hit pointing at a method
/// somebody had just deleted, with nothing in the answer to say so. A thin
/// answer looks thin; a confidently wrong one does not.
///
/// It reindexes rather than probing-then-maybe-reindexing, because indexing
/// *is* the probe: [`index`] folds the file map and returns without writing
/// when the fold matches. Nothing is ever dropped — blob facts and purchased
/// summaries are content-keyed, so an orphaned row is harmless after a reindex
/// and valuable after a revert. Only the checkout's view of them moves.
///
/// **Measured on rails** (3,307 files, 54k units, load ~1.9 on 8 cores): 50 ms
/// when nothing moved, 300 ms after one edited file — one parse plus the map
/// rewrite — against a warm `search` of that corpus at 5.4 s. Re-embedding a
/// changed file's identifiers is milliseconds each and already happens on the
/// query path.
///
/// Every command that answers **from** the index goes through here, so a stale
/// answer is not something a new command can forget to prevent. `--status` is
/// the deliberate exception: its job is to report staleness, not to resolve it.
pub fn open(path: &Path) -> Result<Opened> {
    let mut store = crate::store::open_default()?;
    let (root, refreshed) = index(&mut store, path)?;
    Ok(Opened {
        store,
        root,
        refreshed,
    })
}

pub fn index(store: &mut Store, path: &Path) -> Result<(String, Indexed)> {
    let root = scan::repo_root(path)?;
    let files = scan::scan(&root)?;
    let root_str = root.to_string_lossy().into_owned();

    // One representative path per blob: a blob is a pure function of its
    // bytes, so reading any of the paths that point at it is the same read.
    let mut representative: HashMap<&Oid, &String> = HashMap::new();
    for (path, oid) in &files {
        representative.entry(oid).or_insert(path);
    }
    let wanted: HashSet<Oid> = files.values().cloned().collect();
    let known = store.known(&wanted)?;
    let todo: Vec<(&Oid, &String)> = representative
        .iter()
        .filter(|(oid, _)| !known.contains(**oid))
        .map(|(oid, path)| (*oid, *path))
        .collect();

    let read: Vec<(Oid, Option<Blob>)> = todo
        .par_iter()
        .map(|(oid, path)| {
            // The extractor is chosen by the path, but reads only bytes — so
            // the same blob still yields the same units wherever it sits, and
            // the layer-1 contract holds.
            let blob = std::fs::read(root.join(path))
                .ok()
                // `language` already said yes, or this path would not be in
                // the map at all.
                .and_then(|bytes| units_at(path, &bytes));
            ((*oid).clone(), blob)
        })
        .collect();

    // A file deleted between the scan and the read leaves the map pointing at
    // a blob nothing can produce. Drop those entries rather than teach the
    // store to tolerate a dangling reference (DEC-010's spirit: the special
    // case that cannot occur needs no handling).
    let mut files = files;
    let mut parsed = Vec::with_capacity(read.len());
    for (oid, blob) in read {
        match blob {
            Some(blob) => parsed.push((oid, blob)),
            None => files.retain(|_, o| *o != oid),
        }
    }

    let counts = store.write(&root_str, &files, parsed)?;
    Ok((root_str, counts))
}
