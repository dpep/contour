//! Text → vector, behind a trait (DEC-005).
//!
//! Two implementations. [`HashEmbedder`] is deterministic, dependency-free
//! feature hashing — not semantically trained, but stable, offline, and enough
//! to exercise the whole pipeline; it is what the tests use and what a build
//! without the `semantic` feature gets. `OnnxEmbedder` runs a real
//! `all-MiniLM-L6-v2` through ONNX Runtime and is the one that makes English
//! queries mean anything.
//!
//! `kind()` and the model id are part of every stored vector's key, so indexes
//! from different embedders sit beside each other rather than overwriting each
//! other — the same rule DEC-005 states, and the reason switching embedders
//! costs a re-embed rather than a corruption.
//!
//! **What gets embedded is the summary, not the code and not the name**
//! (DEC-004). The name is the lexical half of the search, and blending it in
//! here would double-count it and blur the very bet this design exists to
//! test: that English summaries answer behavioural queries.

pub mod mrl;
#[cfg(feature = "_semantic")]
mod onnx;

use crate::hash::{FNV_OFFSET, SEP, fnv1a};
use crate::summary::Summary;
use mrl::MRL_DIMS;

/// Native width of the [`HashEmbedder`] fallback. The MRL stage only needs at
/// least [`mrl::MRL_DIMS`] coordinates, so embedders may differ in width.
pub const EMBED_DIMS: usize = 384;

/// Produces an embedding of length ≥ [`mrl::MRL_DIMS`].
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Vec<f32>;

    /// `"hash"` or `"onnx"`. Part of every vector's cache key.
    fn kind(&self) -> &'static str;

    /// The specific model, where there is one. Also part of the key, so two
    /// ONNX models do not overwrite each other's vectors.
    fn model(&self) -> &str {
        ""
    }
}

/// What an embedder is about to be used for.
///
/// Not a knob for its own sake: the right answer is opposite for the two
/// cases. gqls measured one query embed at 21.9 ms single-threaded against
/// 8.7 ms on four, but a whole-corpus embed at 121 s single-threaded against
/// 133 s on four — because rayon is already running an inference per core and
/// intra-op threads on top of that only oversubscribe. Getting this backwards
/// costs about 19% on the bulk path, silently.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Workload {
    /// One query at a time: nothing else is running, so give ONNX the cores.
    Query,
    /// A whole corpus under rayon: one intra-op thread.
    Bulk,
}

/// The best embedder available: the real model if it loads, else the hash
/// fallback. Callers never branch on which one they got — they read `kind()`
/// and disclose it.
pub fn default_embedder(model: Option<&str>, workload: Workload) -> Box<dyn Embedder> {
    #[cfg(feature = "_semantic")]
    if let Some(embedder) = onnx::OnnxEmbedder::load(model, workload.into()) {
        return Box::new(embedder);
    }
    #[cfg(not(feature = "_semantic"))]
    let _ = (model, workload);
    Box::new(HashEmbedder)
}

#[cfg(feature = "_semantic")]
impl From<Workload> for onnx::Workload {
    fn from(workload: Workload) -> onnx::Workload {
        match workload {
            Workload::Query => onnx::Workload::Query,
            Workload::Bulk => onnx::Workload::Bulk,
        }
    }
}

/// Deterministic embedder via signed feature hashing over word tokens.
#[derive(Default, Clone, Copy, Debug)]
pub struct HashEmbedder;

impl Embedder for HashEmbedder {
    fn kind(&self) -> &'static str {
        "hash"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBED_DIMS];
        for token in tokenize(text) {
            let h = fnv1a(FNV_OFFSET, token.as_bytes());
            // Hash into the leading MRL_DIMS coordinates only: the MRL stage
            // keeps that prefix, so spreading over the full width would throw
            // away most of the signal and collapse short texts to zero.
            let idx = (h % MRL_DIMS as u64) as usize;
            let sign = if (h >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += sign;
        }
        v
    }
}

/// Lowercased alphanumeric tokens.
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
}

/// The natural-language text a unit is found by.
///
/// Summary first because it carries the behaviour; purpose, concerns and
/// domain after it because they are the facets a query may name directly. No
/// identifiers: see the module header.
pub fn summary_text(summary: &Summary) -> String {
    let mut text = summary.summary.clone();
    text.push_str(". ");
    text.push_str(&summary.primary_purpose);
    if !summary.secondary_concerns.is_empty() {
        text.push_str(". also ");
        text.push_str(&summary.secondary_concerns.join(", "));
    }
    text.push_str(". domain: ");
    text.push_str(&summary.domain);
    text
}

/// Identity of everything that invalidates a vector other than the text
/// itself: width, embedder, model. Vectors sharing this are interchangeable by
/// content key.
pub fn config_key(kind: &str, model: &str) -> u64 {
    let mut h = fnv1a(FNV_OFFSET, &(MRL_DIMS as u32).to_le_bytes());
    h = fnv1a(h, SEP);
    h = fnv1a(h, kind.as_bytes());
    h = fnv1a(h, SEP);
    fnv1a(h, model.as_bytes())
}

/// Hash of the exact text embedded, so the key can never drift from the
/// content — improve the text and the key moves with it (gqls's law again).
pub fn text_key(text: &str) -> u64 {
    fnv1a(FNV_OFFSET, text.as_bytes())
}

/// Split an identifier into words, so a tokenizer trained on English sees
/// words rather than subword debris. `unpaid_for` → `unpaid for`,
/// `HTTPClient` → `HTTP Client`.
pub fn humanize(ident: &str) -> String {
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        // A word starts where lowercase meets uppercase, or where a run of
        // capitals ends and a word begins — `URLSlug` breaks once, not twice.
        let starts_word = i > 0
            && c.is_uppercase()
            && (chars[i - 1].is_lowercase()
                || chars[i - 1].is_numeric()
                || chars.get(i + 1).is_some_and(|n| n.is_lowercase()));
        if starts_word && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(c);
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_identifiers_into_words() {
        assert_eq!(humanize("unpaid_for"), "unpaid for");
        assert_eq!(humanize("markPaid"), "mark Paid");
        assert_eq!(humanize("Invoice#unpaid_for"), "Invoice unpaid for");
        assert_eq!(humanize("URLSlug"), "URL Slug");
        assert_eq!(humanize("id"), "id");
    }

    #[test]
    fn the_hash_embedder_is_deterministic_and_discriminating() {
        let e = HashEmbedder;
        assert_eq!(e.embed("unpaid invoices").len(), EMBED_DIMS);
        assert_eq!(e.embed("unpaid invoices"), e.embed("unpaid invoices"));
        assert_ne!(e.embed("unpaid invoices"), e.embed("payroll runs"));
    }

    /// The key must move when anything about the embedding setup moves, or a
    /// vector from one model is served as if it came from another.
    #[test]
    fn the_config_key_separates_embedders_and_models() {
        assert_ne!(config_key("hash", ""), config_key("onnx", ""));
        assert_ne!(config_key("onnx", "a"), config_key("onnx", "b"));
        // Adjacent fields must not alias: ("on","nx") is not ("onn","x").
        assert_ne!(config_key("on", "nx"), config_key("onn", "x"));
    }

    #[test]
    fn embedded_text_carries_meaning_and_not_identifiers() {
        let summary = Summary {
            summary: "Returns a customer's unpaid invoices.".into(),
            primary_purpose: "invoice lookup".into(),
            secondary_concerns: vec!["ordering".into()],
            side_effects: Vec::new(),
            domain: "billing".into(),
            patterns: vec!["scope".into()],
        };
        let text = summary_text(&summary);
        assert!(text.contains("unpaid invoices"));
        assert!(text.contains("invoice lookup"));
        assert!(text.contains("also ordering"));
        assert!(text.contains("domain: billing"));
    }
}
