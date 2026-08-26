//! The Anthropic Messages API, over raw HTTP.
//!
//! Rust has no official Anthropic SDK, so this is `POST /v1/messages` by hand
//! — the documented raw-HTTP surface, not a port of another language's client.
//!
//! The response is constrained with `output_config.format`, so the model
//! cannot return prose where a `Summary` is expected and there is no
//! best-effort JSON scraping anywhere below. Everything about the request that
//! shapes an answer lives in `super::prompt` and is covered by that module's
//! frozen test.

use super::prompt;
use super::{Request, Summarizer, Summary, Usage};
use anyhow::{Result, anyhow, bail};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Retries, and the wait before each. Exponential, and short enough that a
/// budgeted run of a few hundred units cannot stall for minutes on one bad
/// unit.
const BACKOFF_MS: [u64; 4] = [500, 1_500, 4_000, 10_000];

pub struct Anthropic {
    api_key: String,
    model: String,
}

impl Anthropic {
    /// Read the key from `ANTHROPIC_API_KEY`.
    ///
    /// Deferred to call time, not build time: contour compiles, tests, and
    /// indexes without a key, and only `summarize` against a live model needs
    /// one.
    pub fn from_env(model: &str) -> Result<Anthropic> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!(
                "ANTHROPIC_API_KEY is not set. Export a key, or pass --fixtures \
                 to replay canned summaries offline."
            )
        })?;
        Ok(Anthropic {
            api_key,
            model: model.to_string(),
        })
    }

    fn body(&self, request: &Request) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": prompt::MAX_TOKENS,
            "system": prompt::SYSTEM,
            "messages": [{
                "role": "user",
                "content": prompt::user_message(&request.context.render(), &request.source),
            }],
            "output_config": {
                "effort": prompt::EFFORT,
                "format": {
                    "type": "json_schema",
                    "schema": prompt::schema(),
                },
            },
        })
    }
}

impl Summarizer for Anthropic {
    fn model(&self) -> &str {
        &self.model
    }

    fn summarize(&self, request: &Request) -> Result<(Summary, Usage)> {
        let body = self.body(request);
        let mut last: Option<anyhow::Error> = None;

        for attempt in 0..=BACKOFF_MS.len() {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(BACKOFF_MS[attempt - 1]));
            }
            match self.post(&body) {
                Ok(response) => return parse(&response),
                Err(err) => {
                    // Only rate limits and server faults are worth repeating.
                    // A 400 is a bug in this file and retrying it just spends
                    // the budget four more times.
                    if !err.retryable {
                        return Err(err.into());
                    }
                    last = Some(err.into());
                }
            }
        }
        Err(last.unwrap_or_else(|| anyhow!("request failed with no error recorded")))
    }
}

/// An HTTP failure, plus whether waiting could fix it.
struct Failure {
    message: String,
    retryable: bool,
}

impl From<Failure> for anyhow::Error {
    fn from(failure: Failure) -> anyhow::Error {
        anyhow!(failure.message)
    }
}

impl Anthropic {
    fn post(&self, body: &serde_json::Value) -> Result<serde_json::Value, Failure> {
        let response = ureq::post(ENDPOINT)
            .set("content-type", "application/json")
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", API_VERSION)
            .send_json(body.clone());

        match response {
            Ok(ok) => ok.into_json().map_err(|err| Failure {
                message: format!("the API returned a body that is not JSON: {err}"),
                retryable: false,
            }),
            Err(ureq::Error::Status(code, response)) => {
                let detail = response
                    .into_string()
                    .unwrap_or_else(|_| "<unreadable>".into());
                Failure {
                    message: format!("HTTP {code} from the Messages API: {}", detail.trim()),
                    retryable: code == 429 || code >= 500,
                }
                .into_err()
            }
            // A transport error — DNS, TLS, a dropped connection. Always worth
            // one more try.
            Err(transport) => Failure {
                message: format!("could not reach the Messages API: {transport}"),
                retryable: true,
            }
            .into_err(),
        }
    }
}

impl Failure {
    fn into_err<T>(self) -> Result<T, Failure> {
        Err(self)
    }
}

/// Pull the summary out of a successful response.
///
/// Split out from the request so it is testable without a network: every
/// branch below is a shape the API really produces, and none of them should
/// reach a user as a panic or as a half-parsed summary.
fn parse(response: &serde_json::Value) -> Result<(Summary, Usage)> {
    // Check why generation stopped *before* reading content. A refusal or a
    // token cutoff both return HTTP 200 with content that does not honour the
    // schema, so trusting `content` first turns a clear failure into a
    // confusing parse error.
    match response["stop_reason"].as_str() {
        Some("end_turn") | None => {}
        Some("refusal") => {
            let category = response["stop_details"]["category"]
                .as_str()
                .unwrap_or("unspecified");
            bail!("the model declined to summarize this method ({category})");
        }
        Some("max_tokens") => bail!(
            "the model ran out of output tokens ({} allowed); the summary would be truncated",
            prompt::MAX_TOKENS
        ),
        Some(other) => bail!("unexpected stop_reason `{other}`"),
    }

    let text = response["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b["type"] == "text")
                .and_then(|b| b["text"].as_str())
        })
        .ok_or_else(|| anyhow!("the response carried no text block"))?;

    let summary: Summary = serde_json::from_str(text)
        .map_err(|err| anyhow!("the response did not match the summary schema: {err}"))?;

    let usage = Usage {
        input_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: response["usage"]["output_tokens"].as_u64().unwrap_or(0),
    };
    Ok((summary, usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(stop_reason: &str, text: &str) -> serde_json::Value {
        serde_json::json!({
            "stop_reason": stop_reason,
            "content": [{"type": "text", "text": text}],
            "usage": {"input_tokens": 812, "output_tokens": 96},
        })
    }

    const GOOD: &str = r#"{"summary":"Returns the customer's unpaid invoices.",
        "primary_purpose":"invoice retrieval","secondary_concerns":["pagination"],
        "side_effects":[],"domain":"billing","patterns":["scope"]}"#;

    #[test]
    fn reads_a_summary_and_what_it_cost() {
        let (summary, usage) = parse(&envelope("end_turn", GOOD)).unwrap();
        assert_eq!(summary.domain, "billing");
        assert_eq!(summary.secondary_concerns, ["pagination"]);
        assert_eq!(usage.input_tokens, 812);
        assert_eq!(usage.output_tokens, 96);
    }

    /// Both of these arrive as HTTP 200 with content that ignores the schema,
    /// so the stop reason has to be read first or the user gets a JSON parse
    /// error instead of the actual problem.
    #[test]
    fn a_refusal_and_a_cutoff_are_reported_as_themselves() {
        let mut refused = envelope("refusal", "I can't help with that.");
        refused["stop_details"] = serde_json::json!({"type": "refusal", "category": "cyber"});
        let err = parse(&refused).unwrap_err().to_string();
        assert!(err.contains("declined") && err.contains("cyber"), "{err}");

        let truncated = parse(&envelope("max_tokens", r#"{"summary":"Returns the"#));
        assert!(
            truncated.unwrap_err().to_string().contains("output tokens"),
            "a cutoff is not a schema failure"
        );
    }

    #[test]
    fn a_body_that_is_not_a_summary_fails_rather_than_half_parsing() {
        let err = parse(&envelope("end_turn", r#"{"summary":"only this"}"#))
            .unwrap_err()
            .to_string();
        assert!(err.contains("did not match the summary schema"), "{err}");
    }

    /// The request is built without a network, so its shape is testable. The
    /// assertions here are the fields whose absence would fail silently: a
    /// missing schema returns prose, a missing effort changes the cost.
    #[test]
    fn the_request_carries_the_schema_and_the_effort() {
        let client = Anthropic {
            api_key: "not-a-key".into(),
            model: "claude-opus-5".into(),
        };
        let body = client.body(&Request {
            source: "def save\n  persist!\nend".into(),
            context: crate::summary::Context {
                name: "save".into(),
                owner: "Widget".into(),
                singleton: false,
                via: None,
                params: Vec::new(),
            },
        });
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(body["output_config"]["effort"], prompt::EFFORT);
        assert_eq!(body["model"], "claude-opus-5");
        let sent = body["messages"][0]["content"].as_str().unwrap();
        assert!(sent.contains("name: save") && sent.contains("persist!"));
    }
}
