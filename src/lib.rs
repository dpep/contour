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
//! - [`ruby`] and [`rust`] — bytes → units. Two modules, one signature; no
//!   other layer learns which produced what (DEC-012). Ruby normalizes a full
//!   AST, Rust hashes a token stream and says so.
//! - [`scan`] — checkout → path→blob map. The only module that knows a path
//!   exists.
//! - [`store`] — units keyed by blob OID (no paths), plus the path map beside
//!   it. N worktrees of one repo cost one index (DEC-003).
//! - [`index`] — the three above, wired together.
//! - [`dupes`] — the exact tier: a group-by over one column.
//! - [`canonical`] — which member of a duplicate group is likely the original,
//!   from signals outside the bodies, each reported with its own pick.
//! - [`near`] — the near-structural tier: Jaccard over subtree signatures,
//!   with an inverted index so nothing is compared pairwise.
//! - [`summary`] — the expensive layer, and the first one that leaves the
//!   machine. Behind a trait, so tests replay canned answers (DEC-006).
//! - [`embed`] — summaries → vectors, behind a trait (DEC-005).
//! - [`search`] — English queries and nearest neighbours, fusing a name match
//!   with a meaning match and disclosing which found what.
//! - [`eval`] — the labeled set that settles every threshold (DEC-011).
//! - [`mcp`] — the agent surface, returning exactly the JSON the CLI does.
//! - [`cancel`] — the flag a served request watches, so abandoning a call
//!   stops the work rather than only the waiting (DEC-031).
//! - [`profile`] — where the wall clock went, for `--profile`.

pub mod cancel;
pub mod canonical;
pub mod cli;
pub mod constants;
pub mod core;
pub mod dupes;
pub mod embed;
pub mod eval;
pub(crate) mod hash;
pub mod index;
pub mod mcp;
pub mod near;
pub mod paths;
pub mod profile;
pub mod ruby;
pub mod rust;
pub mod scan;
pub mod search;
pub mod store;
pub mod summary;
