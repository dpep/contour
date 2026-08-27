//! A summarizer that replays canned answers.
//!
//! Two jobs. It is what the tests use, because no test may make a live API
//! call — a suite whose cost and result depend on the network is a suite
//! nobody runs. And it is what offline development uses, so the whole pipeline
//! downstream of summarization can be exercised without a key.
//!
//! Fixtures are keyed by `Owner#method`, not by the cache key, because a
//! person writes them by hand and `Widget#save` is writable where
//! `a3f1…-9c02…` is not.

use super::{Request, Summarizer, Summary, Usage};
use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;
use std::path::Path;

pub struct Fixtures {
    summaries: HashMap<String, Summary>,
}

impl Fixtures {
    /// Load a JSON object mapping `Owner#method` to a summary.
    pub fn load(path: &Path) -> Result<Fixtures> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading fixtures from {}", path.display()))?;
        let summaries: HashMap<String, Summary> = serde_json::from_str(&text)
            .with_context(|| format!("parsing fixtures from {}", path.display()))?;
        Ok(Fixtures { summaries })
    }
}

impl Summarizer for Fixtures {
    /// Part of the cache key, so replayed summaries can never be mistaken for
    /// real ones — they sit in their own corner of the table.
    fn model(&self) -> &str {
        "fixture"
    }

    fn summarize(&self, request: &Request) -> Result<(Summary, Usage)> {
        let id = request.context.id();
        match self.summaries.get(&id) {
            // Zero usage, honestly: nothing was spent, and reporting a made-up
            // token count would put fiction into the run's totals.
            Some(summary) => Ok((summary.clone(), Usage::default())),
            // Loudly, rather than inventing a placeholder. A missing fixture
            // is a test that would otherwise pass while asserting nothing.
            None => bail!("no fixture for {id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::Context;

    /// `label` must be unique per test: the suite runs them in threads in one
    /// process, and a shared filename makes two tests overwrite each other.
    fn write(label: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "contour-fixtures-{}-{label}.json",
            std::process::id()
        ));
        std::fs::write(&path, body).unwrap();
        path
    }

    fn request(owner: &str, name: &str, singleton: bool) -> Request {
        typed(crate::core::Lang::Ruby, owner, name, singleton)
    }

    fn typed(lang: crate::core::Lang, owner: &str, name: &str, singleton: bool) -> Request {
        Request {
            source: "def save; end".into(),
            context: Context {
                lang,
                name: name.into(),
                owner: owner.into(),
                singleton,
                via: None,
                params: Vec::new(),
            },
        }
    }

    /// A Rust `fn` is looked up under the name Rust writes. The fixture path
    /// used to rebuild the id itself and reach for `Widget#total`, which no
    /// surface prints and nobody would put in a fixture file.
    #[test]
    fn replays_a_rust_fn_by_its_rust_name() {
        let path = write(
            "rust-name",
            r#"{"Widget::total": {"summary":"s","primary_purpose":"p",
                "secondary_concerns":[],"side_effects":[],"domain":"d","patterns":[]}}"#,
        );
        let fixtures = Fixtures::load(&path).unwrap();
        let request = typed(crate::core::Lang::Rust, "Widget", "total", false);
        assert!(fixtures.summarize(&request).is_ok());
    }

    #[test]
    fn replays_by_the_name_a_person_writes() {
        let path = write(
            "ruby-name",
            r#"{"Widget#save": {"summary":"Saves it.","primary_purpose":"persistence",
                "domain":"inventory","side_effects":["persists"]}}"#,
        );
        let fixtures = Fixtures::load(&path).unwrap();
        let (summary, usage) = fixtures
            .summarize(&request("Widget", "save", false))
            .unwrap();
        assert_eq!(summary.domain, "inventory");
        assert_eq!(usage, Usage::default(), "nothing was spent");

        let missing = fixtures.summarize(&request("Widget", "save", true));
        assert!(missing.is_err(), "Widget.save is not Widget#save");
        let _ = std::fs::remove_file(&path);
    }
}
