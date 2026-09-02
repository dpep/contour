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

pub mod fill;
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

/// What this binary can do, decided at compile time.
///
/// Two contours built from the same commit answer English differently, and
/// nothing about them looks different: a default build embeds with a feature
/// hash and matches on names alone. `search` discloses which embedder answered,
/// but only once you have run one — and an install can quietly replace a
/// semantic build with a default one, which is how a session came to spend an
/// afternoon wondering why the index had got worse. `--version` and `--status`
/// say it before you ask a question rather than after.
///
/// Build-time, not resolved: a dynamic build can still fall back at runtime if
/// no system ONNX Runtime is there, and saying so is the honest wording.
#[cfg(feature = "semantic")]
pub const BUILD: &str = "onnx embedder, statically linked";
#[cfg(all(feature = "semantic-dynamic", not(feature = "semantic")))]
pub const BUILD: &str = "onnx embedder, dlopened from a system ONNX Runtime — falls back to the hash embedder if none is found";
#[cfg(not(feature = "_semantic"))]
pub const BUILD: &str =
    "hash embedder — English search matches names, not meaning; rebuild with --features semantic";

/// The best embedder available: the real model if it loads, else the hash
/// fallback. Callers never branch on which one they got — they read `kind()`
/// and disclose it.
pub fn default_embedder(model: Option<&str>, workload: Workload) -> Box<dyn Embedder> {
    // Loading an ONNX session is a fixed cost every command pays before it
    // reads a row, and on a warm scoped query it is a fifth of the wall clock.
    // Named rather than left in the remainder, because a fixed cost that big
    // is a design fact, not noise.
    //
    // **Only the query embedder.** `embed_all` builds one of these per rayon
    // worker, inside the `embed` phase and on eight threads at once — timing
    // those would nest a phase inside a phase and report shares that add to
    // more than the run. `Workload` already tells the two apart.
    let _span = match workload {
        Workload::Query => Some(crate::profile::span("embedder load")),
        Workload::Bulk => None,
    };
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

/// Embed many texts at once, one embedder per worker thread.
///
/// Adapted from gqls's `Session::rank`, including the trap it documents:
/// rayon's `map_init` rebuilds its state per **job split** — roughly once per
/// item — which reloads the ONNX session tens of thousands of times on a large
/// corpus. A `thread_local` caps it at one per worker, which is the entire
/// point, because a session is expensive to build and the vendored embedder
/// holds exactly one behind a mutex.
///
/// That mutex is also why this cannot simply parallelise around a shared
/// `&dyn Embedder`: every thread would serialise on it. One embedder per
/// thread is what actually buys the parallelism.
///
/// How long a cold corpus takes to embed, before anybody waits for it.
///
/// **Measured, per thread, so the estimate follows the machine.** A build
/// corpus of replicated public Ruby was embedded at two sizes on 8 threads
/// (release, 8 cores, quiet machine — `docs/PLAN.md` has the method and the
/// table):
///
/// | embedder | 8 threads | per thread |
/// | -------- | --------: | ---------: |
/// | `onnx`   |     295/s |       37/s |
/// | `hash`   |  8,500,000/s |  1,060,000/s |
///
/// Two significant figures, which is what two points over a 1.9× range support.
/// Per-thread because core count is the variance this can actually see; machine
/// speed it cannot, so the estimate is worth a factor of two or so and is
/// quoted as "about". That is ample for the judgment it is used for, which is
/// telling three minutes from two hours.
///
/// An unknown embedder gets the slow rate. Over-estimating the bill costs a
/// refusal somebody can override; under-estimating it costs the wait this
/// exists to prevent.
fn per_thread_per_second(kind: &str) -> f64 {
    match kind {
        "onnx" => 37.0,
        "hash" => 1_060_000.0,
        _ => 37.0,
    }
}

/// Seconds this many texts should take to embed on this machine.
pub fn projected_seconds(kind: &str, texts: usize) -> f64 {
    let rate = per_thread_per_second(kind) * rayon::current_num_threads().max(1) as f64;
    texts as f64 / rate
}

/// How long a caller is willing to wait for a corpus to be embedded before
/// their first answer, in seconds. `CONTOUR_EMBED_BUDGET` overrides it; `0`
/// removes it.
///
/// **Five minutes, and both ends of that are evidence.** A cold rails checkout
/// — 54k units, the corpus this tool was designed against and the one its docs
/// call a one-time cost — is about three minutes, so the budget has to sit
/// above it or the documented normal becomes a refusal. A 2M-unit monorepo is
/// about two hours, which a field report reached and abandoned after twenty
/// minutes. Anything between those two separates them; five minutes is the
/// round number in the gap.
///
/// It is a **budget, not a limit on the corpus**: scoping the same query to a
/// directory at a time warms the index cumulatively, and every one of those
/// answers arrives. That is why refusing is better than starting — the caller
/// gets a shorter first answer rather than a longer wait.
fn budget_seconds() -> Option<f64> {
    let budget = std::env::var("CONTOUR_EMBED_BUDGET")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(300.0);
    (budget > 0.0).then_some(budget)
}

/// Refuse a cold corpus that would cost more than the budget, saying what it
/// weighed and what to do instead.
///
/// Placed where the bill is known and before any of it is paid. The numbers are
/// in the message rather than only in a doc because this is the one moment a
/// caller can act on them — DEC-032.
pub fn afford(kind: &str, texts: usize, units_in_scope: usize) -> anyhow::Result<()> {
    let Some(budget) = budget_seconds() else {
        return Ok(());
    };
    let projected = projected_seconds(kind, texts);
    if projected <= budget {
        return Ok(());
    }
    anyhow::bail!(
        "this scope holds {units_in_scope} unit(s), {texts} of which have nothing embedded \
         yet. That is about {} with the {kind} embedder on {} thread(s), against a budget of \
         {}. Narrow the scope — a directory at a time warms the index for good, and each \
         answer arrives — or run `contour embed` on it once, which pays the same bill \
         deliberately, with progress, and keeps it. `CONTOUR_EMBED_BUDGET` sets the budget \
         in seconds (0 for no budget).",
        about(projected),
        rayon::current_num_threads().max(1),
        about(budget),
    )
}

/// A duration at the precision an estimate built from a rate actually has.
pub(crate) fn about(seconds: f64) -> String {
    match seconds {
        s if s < 120.0 => format!("{} second(s)", s.round() as u64),
        s => format!("{} minute(s)", (s / 60.0).round() as u64),
    }
}

/// `Workload::Bulk` on purpose: rayon is already running an inference per
/// core, so intra-op threads on top of that only oversubscribe — measured by
/// gqls at ~19% slower when set the other way.
///
/// Order-preserving, so the result stays index-aligned with `texts`.
pub fn embed_all(spec: Option<&str>, texts: &[String]) -> Vec<Vec<f32>> {
    use std::cell::RefCell;

    thread_local! {
        static LOCAL: RefCell<Option<Box<dyn Embedder>>> = const { RefCell::new(None) };
    }
    // Taken here and captured, never read inside the closure: a rayon worker is
    // a different thread and would find its own never-cancelled token
    // (`crate::cancel`). This is the loop that burns every core on a cold
    // corpus, so it is the one that has to stop.
    let cancel = crate::cancel::current();
    let mut span = crate::profile::span("embed");
    span.note(|| format!("{} text(s)", texts.len()));
    crate::profile::count("texts embedded", texts.len() as u64);
    // `spec` is constant for a process, so every worker resolves the same
    // embedder kind and no run can mix onnx and hash vectors into one index.
    crate::pool::map(texts, |text| {
        // Not an early exit — rayon has no break — but each remaining item
        // becomes O(1), so the pool drains in about the time one embedding
        // takes. The caller must treat the result as void, which is what
        // `cancel.check()` beside every call site does.
        if cancel.cancelled() {
            return Vec::new();
        }
        LOCAL.with(|cell| {
            let mut slot = cell.borrow_mut();
            let embedder = slot.get_or_insert_with(|| default_embedder(spec, Workload::Bulk));
            mrl::compress_matryoshka_vector(&embedder.embed(text))
        })
    })
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
///
/// The one splitter: the lexical half of search and the eval baseline score
/// against these same tokens, so a change to the rule has to move all three
/// together or the halves stop agreeing on what a word is.
pub fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
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

/// The natural-language text a unit is found by when nothing has summarized it.
///
/// Names, humanized: the owner path, the method name, and the parameter names,
/// split into words so a model trained on English sees words rather than
/// subword debris. `Invoice#unpaid_for(customer)` becomes
/// `Invoice unpaid for customer`.
///
/// **This tier captures what code is CALLED, not what it DOES**, and that is
/// its honest limitation. It answers "where is the invoice settling logic"
/// well and "which methods retry on failure" badly, because the second is
/// never in a name. It is exactly blind where names are bad — which is where a
/// summary earns its cost.
///
/// Comments are deliberately excluded. Folding them in would make the tier
/// better on well-commented code and silently unfalsifiable about which half
/// of the signal was doing the work, and DEC-008 already reserves
/// comment-derived text for a variant of its own.
pub fn identifier_text(unit: &crate::core::Unit) -> String {
    let mut parts = Vec::new();
    if !unit.owner.is_empty() {
        parts.push(humanize(&unit.owner));
    }
    parts.push(humanize(&unit.name));
    for param in &unit.params {
        parts.push(humanize(&param.name));
    }
    parts.join(" ")
}

/// Which text a unit is found by, where it has both a summary and a name.
///
/// `Best` is what every caller but the eval wants. `IdentifierOnly` exists so
/// the eval can score the tiers against each other on one corpus — the
/// embed-code-against-embed-summary comparison DEC-004 promised — without
/// needing two indexes to do it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prefer {
    Best,
    IdentifierOnly,
}

/// The text a unit is embedded as, and the key that text is stored under.
pub struct Text {
    pub key: u64,
    pub text: String,
    /// Which tier produced it: `summary` or `identifier`.
    pub via: &'static str,
}

/// The one rule for what a unit is embedded as.
///
/// Both the query path and the fill ask this, which is what makes a vector
/// `embed` bought the one `search` looks for: two loops choosing the text
/// separately would agree until one of them was edited. `None` where there is
/// nothing to embed — a macro-generated unit with no name text at all.
pub fn text_of(
    unit: &crate::core::Unit,
    summary: Option<&Summary>,
    prefer: Prefer,
) -> Option<Text> {
    // Prefer meaning over naming where both exist — DEC-005's interop rule.
    let (text, via) = match (summary, prefer) {
        (Some(summary), Prefer::Best) => (summary_text(summary), "summary"),
        _ => (identifier_text(unit), "identifier"),
    };
    if text.trim().is_empty() {
        return None;
    }
    let key = text_key(&text);
    Some(Text { key, text, via })
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

    /// The pool that burns every core on a cold corpus stops when the request
    /// that started it is cancelled — and the ambient token really does reach
    /// the rayon workers, which is the one sharp edge in `crate::cancel`.
    ///
    /// The second half is the control: without it, "every vector is empty"
    /// would also pass if `embed_all` had simply stopped working.
    #[test]
    fn a_cancelled_embedding_stops_rather_than_finishing() {
        let texts: Vec<String> = (0..64).map(|i| format!("widget handler {i}")).collect();

        let cancel = crate::cancel::Cancel::new();
        cancel.cancel();
        let stopped = crate::cancel::with(&cancel, || embed_all(None, &texts));
        assert_eq!(stopped.len(), texts.len(), "one slot per text, still");
        assert!(stopped.iter().all(Vec::is_empty), "it kept embedding");

        assert!(embed_all(None, &texts).iter().all(|v| !v.is_empty()));
    }

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
    fn identifier_text_reads_as_words() {
        let unit = crate::core::Unit {
            lang: crate::core::Lang::Ruby,
            name: "unpaid_for".into(),
            owner: "Billing::Invoice".into(),
            singleton: false,
            visibility: Default::default(),
            nodes: None,
            params: vec![crate::core::Param {
                kind: crate::core::ParamKind::Req,
                name: "customer_id".into(),
            }],
            via: None,
            line: 1,
            end_line: 3,
            norm_hash: None,
        };
        assert_eq!(
            identifier_text(&unit),
            "Billing Invoice unpaid for customer id"
        );
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
