//! The request, minus the method. Everything here shapes the answer, so
//! everything here is pinned by one frozen test.
//!
//! `PROMPT_VERSION` is part of the summary cache key. The trap it guards
//! against is editing the instructions — or the effort, or the schema — while
//! leaving the version alone, which serves answers no longer reachable from
//! the prompt that supposedly produced them. `the_request_shape_is_frozen`
//! below hashes all of it together, so any such edit fails the build and says
//! to bump the version. Same idiom as `crate::hash` and `ruby::norm`.

/// Bump whenever anything in this file changes. See the frozen test.
///
/// v2 dropped `other` from the side-effect enum and glossed `raises` for
/// languages that return their errors. Free to take now and not later: no API
/// fill has ever run, so no purchased answer is keyed under v1 (DEC-017's
/// reasoning, at its own moment).
pub const PROMPT_VERSION: &str = "v2";

/// Thinking depth. A per-method summary is a bounded, well-specified task, and
/// this is the dominant cost lever after model choice — 54k methods times a
/// few thousand thinking tokens is real money. `low` is the starting guess;
/// DEC-006's bake-off settles it against the eval set, and moving it is a
/// version bump because it changes the answer.
pub(crate) const EFFORT: &str = "low";

/// Generous for an object this small. Thinking tokens count against it, which
/// is the reason it is not 1024.
pub(crate) const MAX_TOKENS: u32 = 8192;

pub(crate) const SYSTEM: &str = "\
You summarize one method of source code at a time, for a semantic index that \
answers behavioural questions like \"which methods retrieve collections of \
domain objects\".

Describe what the method does for its caller and what it changes in the world. \
Write for a reader who cannot see the code.

- Do not name local variables, and do not restate the syntax. \"Iterates an \
array\" is worthless; \"returns the unpaid invoices for a customer, newest \
first\" is the job.
- primary_purpose is the one reason this method exists. Exactly one.
- secondary_concerns are ranked, most significant first, and are usually empty. \
A concern is secondary when the method would still make sense without it: \
pagination inside a payroll query is secondary, the payroll is primary.
- domain is the business area in the codebase's own vocabulary, lowercase. Use \
\"unknown\" when the method is generic plumbing with no domain.
- Judge only from what you are shown. Do not guess at what the methods it \
calls do; if a name is opaque, say what the method does with the result.";

/// The instruction wrapper around one method. `{context}` and `{source}` are
/// the only substitutions.
pub(crate) const TEMPLATE: &str = "\
Summarize this method.

<method>
{context}
</method>

<source>
{source}
</source>";

pub(crate) fn user_message(context: &str, source: &str) -> String {
    TEMPLATE
        .replace("{context}", context.trim_end())
        .replace("{source}", source)
}

/// The JSON schema the response is constrained to.
///
/// `additionalProperties: false` is required by the API for every object, and
/// every property must appear in `required` — a structured output has no
/// optional fields, so "no secondary concerns" is an empty array rather than
/// an absent key.
pub(crate) fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": {
                "type": "string",
                "description": "One or two sentences: what the caller gets, and what changes as a result."
            },
            "primary_purpose": {
                "type": "string",
                "description": "The single reason this method exists, as a short noun phrase."
            },
            "secondary_concerns": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Ranked, most significant first. Empty when the method does one thing."
            },
            "side_effects": {
                "type": "array",
                "items": {"type": "string", "enum": super::SIDE_EFFECTS},
                "description": "What this does besides return a value. Empty for a pure function. \
                    `raises` means it signals failure to its caller as part of its contract — a \
                    Ruby raise, a Rust Err return, a documented panic."
            },
            "domain": {
                "type": "string",
                "description": "Business area in the codebase's vocabulary, lowercase, or \"unknown\"."
            },
            "patterns": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Recognised implementation patterns, e.g. \"memoization\", \"guard clause\"."
            }
        },
        "required": ["summary", "primary_purpose", "secondary_concerns",
                     "side_effects", "domain", "patterns"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{FNV_OFFSET, SEP, fnv1a};

    /// Frozen: everything that shapes an answer, hashed together.
    ///
    /// If this fails you changed the instructions, the schema, the effort, or
    /// the token ceiling. That is fine — **bump `PROMPT_VERSION` and update
    /// the constant below**. Do not update the constant alone: stored
    /// summaries would then be served under a prompt that no longer produces
    /// them, which is the exact drift the version exists to prevent.
    #[test]
    fn the_request_shape_is_frozen() {
        let mut h = fnv1a(FNV_OFFSET, SYSTEM.as_bytes());
        h = fnv1a(h, SEP);
        h = fnv1a(h, TEMPLATE.as_bytes());
        h = fnv1a(h, SEP);
        h = fnv1a(h, EFFORT.as_bytes());
        h = fnv1a(h, SEP);
        h = fnv1a(h, &MAX_TOKENS.to_le_bytes());
        h = fnv1a(h, SEP);
        h = fnv1a(h, schema().to_string().as_bytes());
        assert_eq!(
            (PROMPT_VERSION, h),
            ("v2", 0x337f_f7d8_f2c2_7717),
            "the request shape changed; bump PROMPT_VERSION with it"
        );
    }

    #[test]
    fn the_method_is_substituted_into_the_template() {
        let filled = user_message("name: save\n", "def save; end\n");
        assert!(filled.contains("name: save\n</method>"));
        assert!(filled.contains("def save; end"));
        assert!(!filled.contains('{'), "no placeholder survives");
    }
}
