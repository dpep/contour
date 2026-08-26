//! contour — a multi-resolution semantic index of source code.
//!
//! The engine is the product; [`cli`] is one consumer of it, and an MCP server
//! or LSP would be another (DEC-002). Read [`core`] first: [`core::Unit`] is
//! the single noun everything here is about, and the chain of keys it
//! accumulates — blob OID, structural hash, summary, embedding — is the whole
//! architecture.
//!
//! Layers, strictly separated:
//!
//! - [`ruby`] — bytes → units. The only Ruby-aware module; a second language
//!   is a second module with the same signature (DEC-012).
//! - [`scan`] — checkout → path→blob map. The only module that knows a path
//!   exists.
//! - [`store`] — units keyed by blob OID (no paths), plus the path map beside
//!   it. N worktrees of one repo cost one index (DEC-003).
//! - [`index`] — the three above, wired together.
//! - [`dupes`] — the first product, and a group-by over one column.
//! - [`summary`] — the expensive layer, and the first one that leaves the
//!   machine. Behind a trait, so tests replay canned answers (DEC-006).

pub mod cli;
pub mod core;
pub mod dupes;
pub(crate) mod hash;
pub mod index;
pub mod paths;
pub mod ruby;
pub mod scan;
pub mod store;
pub mod summary;
