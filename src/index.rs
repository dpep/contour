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
            let blob = std::fs::read(root.join(path)).ok().map(|bytes| {
                match crate::scan::language(path) {
                    Some(Lang::Rust) => crate::rust::units(&bytes),
                    // `language` already said yes, or this path would not be
                    // in the map at all.
                    _ => crate::ruby::units(&bytes),
                }
            });
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

    let git_state = scan::git_fingerprint(&root).unwrap_or(0);
    let counts = store.write(&root_str, &files, parsed, git_state)?;
    Ok((root_str, counts))
}
