//! What a method *means*, as structured data.
//!
//! This is the expensive layer, and the first one that leaves the machine. Two
//! things follow from that and shape everything here.
//!
//! **The key derives from the exact input** (gqls's law, DEC-003). A summary is
//! stored under `norm_hash + ctx_hash + prompt_version + model`, where
//! `ctx_hash` is a hash of the very text the prompt renders — not of the fields
//! it was built from. Improve the rendering and the key moves with it, so a
//! stale summary cannot be served under a prompt that would no longer produce
//! it. `norm_hash` is the one deliberate looseness: it is blind to local
//! variable names, because DEC-003 decided a rename is not a change.
//!
//! **The output is typed, not prose** (DEC-007). A bag of equal keywords
//! flattens a pagination concern inside a payroll method into a peer of
//! "payroll". Ranked secondary concerns and a closed side-effect vocabulary
//! keep that distinction, and they are what the Phase-2 metadata facets filter
//! on.

pub mod anthropic;
pub mod contributed;
pub(crate) mod fill;
pub mod fixture;
mod prompt;

pub use fill::{Coverage, Filled, Pending, coverage, fill, pending};

use crate::core::{Param, Unit};
use crate::hash::{FNV_OFFSET, fnv1a};
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub use prompt::PROMPT_VERSION;

/// What one method means. One LLM call produces exactly this.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// One or two sentences of behaviour: what a caller gets, and what changes
    /// as a result. This is the text that gets embedded (DEC-004).
    pub summary: String,
    /// The single thing this method exists to do.
    pub primary_purpose: String,
    /// Ranked, most significant first, and often empty. Ranking is the point:
    /// a pagination concern inside a payroll method stays secondary rather
    /// than becoming a peer of "payroll" (DEC-007).
    #[serde(default)]
    pub secondary_concerns: Vec<String>,
    #[serde(default)]
    pub side_effects: Vec<SideEffect>,
    /// The business domain, in the corpus's own words — "payroll", "auth".
    /// Free text because the vocabulary is the codebase's, not ours.
    pub domain: String,
    /// Recognised implementation patterns — "memoization", "guard clause".
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// What a method does besides return a value.
///
/// A closed vocabulary, unlike `domain` and `patterns`, because the set really
/// is enumerable and an open one would drift into synonyms — "writes to db",
/// "persists", "saves record" as three separate facets of one fact. `Other`
/// catches anything the model invents, so an unexpected string degrades one
/// field instead of failing the whole parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    /// Writes to a database or other durable store.
    Persists,
    /// Talks to something over a network.
    Network,
    /// Reads or writes the filesystem.
    Filesystem,
    /// Mutates state reachable by the caller — the receiver, an argument, a
    /// global.
    Mutates,
    /// Emits logs, metrics, or traces.
    Observes,
    /// Raises as part of its contract, rather than only on a bug.
    Raises,
    /// Starts or signals a process, thread, or job.
    Spawns,
    #[serde(other)]
    Other,
}

/// Everything the prompt says about a method *other* than its source.
///
/// The fields here are exactly the fields [`Context::render`] writes, and
/// `render`'s output is what [`Context::hash`] hashes. Adding a field without
/// rendering it changes nothing; rendering one without adding it is impossible.
/// That is the whole reason this type exists rather than passing a `&Unit`.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    /// Not rendered into the prompt's identifying lines, and so **not part of
    /// `ctx_hash`** — `norm_hash` is already language-seeded, so two languages
    /// can never reach the same summary key anyway. It is here so the context
    /// can name its own unit the way every other surface does.
    pub lang: crate::core::Lang,
    pub name: String,
    pub owner: String,
    pub singleton: bool,
    pub via: Option<String>,
    pub params: Vec<Param>,
}

impl Context {
    pub fn of(unit: &Unit) -> Context {
        Context {
            lang: unit.lang,
            name: unit.name.clone(),
            owner: unit.owner.clone(),
            singleton: unit.singleton,
            via: unit.via.clone(),
            params: unit.params.clone(),
        }
    }

    /// The context block, as the prompt shows it to the model.
    ///
    /// A summary of a nameless body is a weaker summary, so this is worth
    /// sending; and because it is sent, it has to be in the cache key.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("name: {}\n", self.name));
        out.push_str(&format!(
            "defined on: {}\n",
            match self.owner.is_empty() {
                true => "(top level)",
                false => &self.owner,
            }
        ));
        // Each language's own words. Ruby's line is byte-identical to what it
        // has always been, so no Ruby summary's `ctx_hash` moves; only Rust's
        // does, and telling a model that `Widget::total` is an "instance
        // method" was a falsehood baked into a purchased artifact.
        out.push_str(&match (self.lang, self.singleton) {
            (crate::core::Lang::Ruby, true) => "kind: class method\n".to_string(),
            (crate::core::Lang::Ruby, false) => "kind: instance method\n".to_string(),
            (crate::core::Lang::Rust, true) => "kind: associated function\n".to_string(),
            (crate::core::Lang::Rust, false) => "kind: method\n".to_string(),
        });
        if let Some(via) = &self.via {
            out.push_str(&format!("generated by: {via}\n"));
        }
        if !self.params.is_empty() {
            let rendered: Vec<String> = self
                .params
                .iter()
                .map(|p| format!("{} ({})", p.name, p.kind.as_str()))
                .collect();
            out.push_str(&format!("parameters: {}\n", rendered.join(", ")));
        }
        out
    }

    /// Hash of the rendered text, not of the fields. Cache and content cannot
    /// drift apart when the key is derived from the content itself.
    pub fn hash(&self) -> u64 {
        fnv1a(FNV_OFFSET, self.render().as_bytes())
    }

    /// This unit, named the way every other surface names it.
    pub fn id(&self) -> String {
        crate::core::id(self.lang, &self.owner, &self.name, self.singleton)
    }
}

/// One method, ready to be summarized.
#[derive(Clone, Debug)]
pub struct Request {
    /// The method's source, `def` through `end`, exactly as written.
    pub source: String,
    pub context: Context,
}

/// What one call cost. Reported rather than priced: per-token rates change,
/// and a hardcoded dollar figure would quietly go wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

/// A source of summaries.
///
/// Deliberately one method at a time. DEC-006 wants the batch API for bulk
/// fills, and a `summarize_batch` shaped for it now would be a guess at an
/// interface nothing calls — the budgeted loop is serial, and rate limits mean
/// it would stay serial even with a batch method available.
pub trait Summarizer {
    /// Identifies this summarizer in the cache key, so summaries from
    /// different models sit beside each other rather than overwrite each other
    /// (DEC-005's rule, applied one layer up).
    fn model(&self) -> &str;

    fn summarize(&self, request: &Request) -> Result<(Summary, Usage)>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ParamKind;

    fn ctx() -> Context {
        Context {
            lang: crate::core::Lang::Ruby,
            name: "save".into(),
            owner: "Widget".into(),
            singleton: false,
            via: None,
            params: vec![Param {
                kind: ParamKind::Keyreq,
                name: "force".into(),
            }],
        }
    }

    /// The property the whole cache rests on: anything the model was told is
    /// in the key, because the key is a hash of what it was told.
    #[test]
    fn the_key_moves_whenever_the_prompt_would() {
        let base = ctx().hash();
        for (label, mutate) in [
            (
                "a rename",
                &(|c: &mut Context| c.name = "persist".into()) as &dyn Fn(&mut Context),
            ),
            ("a move", &|c: &mut Context| c.owner = "Gadget".into()),
            ("a singleton", &|c: &mut Context| c.singleton = true),
            ("a macro", &|c: &mut Context| {
                c.via = Some("module_function".into())
            }),
            ("a parameter", &|c: &mut Context| c.params.clear()),
        ] {
            let mut other = ctx();
            mutate(&mut other);
            assert_ne!(base, other.hash(), "{label} must move the context hash");
        }
    }

    #[test]
    fn the_rendered_context_reads_as_prose() {
        let rendered = ctx().render();
        assert!(rendered.contains("name: save"));
        assert!(rendered.contains("defined on: Widget"));
        assert!(rendered.contains("kind: instance method"));
        assert!(rendered.contains("force (keyreq)"));
        assert!(!rendered.contains("generated by"), "no macro here");
    }

    /// An unfamiliar side effect degrades one field rather than failing the
    /// parse — the model inventing a word must not lose the whole summary.
    #[test]
    fn an_unknown_side_effect_falls_back_rather_than_failing() {
        let parsed: Summary = serde_json::from_str(
            r#"{"summary":"s","primary_purpose":"p","domain":"d",
                "side_effects":["persists","teleports"]}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.side_effects,
            [SideEffect::Persists, SideEffect::Other]
        );
        assert!(parsed.patterns.is_empty(), "absent lists default to empty");
    }
}
