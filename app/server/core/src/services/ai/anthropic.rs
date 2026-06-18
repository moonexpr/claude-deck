/// Anthropic Messages API client.
///
/// Two entry points:
///   stream_messages  — returns a Stream of AnthropicEvent (parsed SSE)
///   complete_messages — returns the full text and token usage synchronously
///
/// Neither function reads environment variables. All config is passed in.
/// As of D7, the credential is an `AuthCredential` (api-key vs bearer) so
/// the OAuth-via-claudecode_ext path can flow through the same code.
use anyhow::{Context, anyhow};
use eventsource_stream::Eventsource;
use futures::stream::{Stream, StreamExt};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use super::Message;
use super::key_provider::AuthCredential;

/// Set the right auth header on a request based on the credential variant.
/// `anthropic-version` and `content-type` are set by the caller separately.
fn apply_auth(req: RequestBuilder, auth: &AuthCredential) -> RequestBuilder {
    match auth {
        AuthCredential::ApiKey(k) => req.header("x-api-key", k),
        AuthCredential::Bearer(b) => req.header("authorization", format!("Bearer {b}")),
    }
}

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
const DEFAULT_MAX_TOKENS: u32 = 4096;

// ---------------------------------------------------------------------------
// Anthropic SSE event types
// ---------------------------------------------------------------------------

/// Parsed representation of an Anthropic SSE event we care about.
#[derive(Debug, Clone)]
pub enum AnthropicEvent {
    /// A text delta from a content_block_delta event.
    TextDelta { text: String },
    /// Carries token usage from `message_start` or `message_delta`.
    /// The proxy accumulates these across the stream so the final `d:` frame
    /// contains accurate counts rather than zeros.
    UsageUpdate { input_tokens: u64, output_tokens: u64 },
    /// The message is complete; the proxy emits the `d:` frame.
    MessageStop,
    /// Any other event type — we skip these.
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// Raw JSON shapes from Anthropic SSE
#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct RawDelta {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct RawContentBlockDelta {
    delta: Option<RawDelta>,
}

#[derive(Deserialize)]
struct RawMessageDelta {
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // mirrors the Anthropic message_start shape; usage parsing is deferred
struct RawMessage {
    usage: Option<RawUsage>,
}

// ---------------------------------------------------------------------------
// Request body builder
// ---------------------------------------------------------------------------

fn build_request_body(model: &str, messages: &[Message], stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": DEFAULT_MAX_TOKENS,
        "stream": stream,
        "messages": messages,
    })
}

// ---------------------------------------------------------------------------
// Streaming path
// ---------------------------------------------------------------------------

/// Call Anthropic with `stream: true` and return a Stream of `AnthropicEvent`.
///
/// Takes owned `String` values so the returned stream is `'static` and can be
/// forwarded directly into an axum `Body` without lifetime issues.
///
/// The stream closes after `AnthropicEvent::MessageStop` is emitted.
pub async fn stream_messages(
    client: Client,
    base_url: String,
    auth: AuthCredential,
    model: Option<String>,
    messages: Vec<Message>,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<AnthropicEvent>>> {
    let model_str = model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = build_request_body(model_str, &messages, true);

    let req = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body);
    let resp = apply_auth(req, &auth)
        .send()
        .await
        .context("failed to connect to Anthropic API")?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        let detail = resp
            .text()
            .await
            .unwrap_or_else(|_| "upstream error".to_string());
        return Err(anyhow!("upstream_error|{}|{}", code, detail));
    }

    // Parse the byte stream as SSE events using eventsource-stream
    let byte_stream = resp.bytes_stream();
    let event_stream = byte_stream.eventsource();

    let parsed = event_stream.map(|item| {
        let event = item.map_err(|e| anyhow!("SSE stream error: {}", e))?;
        parse_event(&event.event, &event.data)
    });

    Ok(parsed)
}

/// Parse a single SSE event (by event type + data string) into an `AnthropicEvent`.
fn parse_event(event_type: &str, data: &str) -> anyhow::Result<AnthropicEvent> {
    match event_type {
        "content_block_delta" => {
            let raw: RawContentBlockDelta =
                serde_json::from_str(data).unwrap_or(RawContentBlockDelta { delta: None });
            if let Some(delta) = raw.delta
                && delta.kind == "text_delta" {
                    return Ok(AnthropicEvent::TextDelta {
                        text: delta.text.unwrap_or_default(),
                    });
                }
            Ok(AnthropicEvent::Other)
        }
        "message_delta" => {
            // message_delta carries cumulative output_tokens in streaming mode.
            let raw: RawMessageDelta =
                serde_json::from_str(data).unwrap_or(RawMessageDelta { usage: None });
            let (input, output) = raw
                .usage
                .map(|u| (u.input_tokens.unwrap_or(0), u.output_tokens.unwrap_or(0)))
                .unwrap_or((0, 0));
            Ok(AnthropicEvent::UsageUpdate { input_tokens: input, output_tokens: output })
        }
        "message_stop" => {
            // usage lives in message_start / message_delta — the proxy accumulates
            // it via UsageUpdate events. We emit a bare MessageStop here; the
            // proxy inserts the accumulated counts into the `d:` frame.
            Ok(AnthropicEvent::MessageStop)
        }
        "message_start" => {
            // Carries initial input_tokens (prompt token count) for the request.
            let raw: RawMessage = serde_json::from_str(data)
                .map(|r: serde_json::Value| RawMessage {
                    usage: r
                        .get("message")
                        .and_then(|m| m.get("usage"))
                        .and_then(|u| serde_json::from_value(u.clone()).ok()).map(|u: RawUsage| RawUsage {
                                input_tokens: u.input_tokens,
                                output_tokens: u.output_tokens,
                            }),
                })
                .unwrap_or(RawMessage { usage: None });
            let (input, output) = raw
                .usage
                .map(|u| (u.input_tokens.unwrap_or(0), u.output_tokens.unwrap_or(0)))
                .unwrap_or((0, 0));
            Ok(AnthropicEvent::UsageUpdate { input_tokens: input, output_tokens: output })
        }
        _ => Ok(AnthropicEvent::Other),
    }
}

// ---------------------------------------------------------------------------
// Non-streaming path (for /suggest)
// ---------------------------------------------------------------------------

/// Call Anthropic with `stream: false` and return the full text + usage.
pub async fn complete_messages(
    client: Client,
    base_url: String,
    auth: AuthCredential,
    model: Option<String>,
    messages: Vec<Message>,
) -> anyhow::Result<(String, Usage)> {
    let model_str = model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = build_request_body(model_str, &messages, false);

    let req = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body);
    let resp = apply_auth(req, &auth)
        .send()
        .await
        .context("failed to connect to Anthropic API")?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        let detail = resp
            .text()
            .await
            .unwrap_or_else(|_| "upstream error".to_string());
        return Err(anyhow!("upstream_error|{}|{}", code, detail));
    }

    #[derive(Deserialize)]
    struct NonStreamResponse {
        content: Vec<ContentBlock>,
        usage: Option<RawUsage>,
    }
    #[derive(Deserialize)]
    struct ContentBlock {
        #[serde(rename = "type")]
        kind: String,
        text: Option<String>,
    }

    let parsed: NonStreamResponse = resp
        .json()
        .await
        .context("failed to parse Anthropic response JSON")?;

    let text = parsed
        .content
        .into_iter()
        .filter(|b| b.kind == "text")
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join("");

    let usage = parsed
        .usage
        .map(|u| Usage {
            input_tokens: u.input_tokens.unwrap_or(0),
            output_tokens: u.output_tokens.unwrap_or(0),
        })
        .unwrap_or(Usage {
            input_tokens: 0,
            output_tokens: 0,
        });

    Ok((text, usage))
}

// ---------------------------------------------------------------------------
// Structured-output path (forced tool-use) — used by the Insight Platform.
// ---------------------------------------------------------------------------

/// Extract the forced-tool-use `input` (schema-valid object) + usage from a
/// non-streaming Anthropic Messages response. Pure so it is unit-testable
/// without a network round-trip.
fn parse_tool_use(
    json: &serde_json::Value,
    tool_name: &str,
) -> anyhow::Result<(serde_json::Value, Usage)> {
    let content = json
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("response missing content array"))?;
    let input = content
        .iter()
        .find(|b| {
            b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                && b.get("name").and_then(|n| n.as_str()) == Some(tool_name)
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("no tool_use block named '{tool_name}' in response"))?;
    let usage = json
        .get("usage")
        .map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .unwrap_or(Usage { input_tokens: 0, output_tokens: 0 });
    Ok((input, usage))
}

/// Call Anthropic with a single tool and `tool_choice` forcing it, returning
/// the tool's `input` (guaranteed schema-valid by the API) plus token usage.
///
/// `system` is sent as the top-level Anthropic `system` field; `messages`
/// should carry only user/assistant turns. Schema-validity is guaranteed by
/// the API — *groundedness is not*, so callers must still run the provenance
/// gate over the returned object (Plan 0004a §M0).
#[allow(clippy::too_many_arguments)]
pub async fn complete_structured(
    client: Client,
    base_url: String,
    auth: AuthCredential,
    model: Option<String>,
    system: Option<String>,
    messages: Vec<Message>,
    tool_name: &str,
    tool_description: &str,
    input_schema: serde_json::Value,
) -> anyhow::Result<(serde_json::Value, Usage)> {
    let model_str = model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": model_str,
        "max_tokens": DEFAULT_MAX_TOKENS,
        "stream": false,
        "messages": messages,
        "tools": [{
            "name": tool_name,
            "description": tool_description,
            "input_schema": input_schema,
        }],
        "tool_choice": { "type": "tool", "name": tool_name },
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

    let req = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body);
    let resp = apply_auth(req, &auth)
        .send()
        .await
        .context("failed to connect to Anthropic API")?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        let detail = resp
            .text()
            .await
            .unwrap_or_else(|_| "upstream error".to_string());
        return Err(anyhow!("upstream_error|{}|{}", code, detail));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse Anthropic response JSON")?;
    parse_tool_use(&json, tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_use_extracts_input_and_usage() {
        let canned = serde_json::json!({
            "content": [
                { "type": "text", "text": "ignored preamble" },
                { "type": "tool_use", "name": "record_session_insight",
                  "input": { "summary": "did X", "judgment_calls": [], "follow_ups": [] } }
            ],
            "usage": { "input_tokens": 1200, "output_tokens": 64 }
        });
        let (input, usage) = parse_tool_use(&canned, "record_session_insight").unwrap();
        assert_eq!(input["summary"], "did X");
        assert!(input["judgment_calls"].is_array());
        assert_eq!(usage.input_tokens, 1200);
        assert_eq!(usage.output_tokens, 64);
    }

    #[test]
    fn parse_tool_use_errors_when_tool_absent() {
        let canned = serde_json::json!({
            "content": [{ "type": "text", "text": "no tool here" }],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        });
        assert!(parse_tool_use(&canned, "record_session_insight").is_err());
    }

    #[test]
    fn parse_tool_use_usage_defaults_to_zero() {
        let canned = serde_json::json!({
            "content": [
                { "type": "tool_use", "name": "t", "input": { "ok": true } }
            ]
        });
        let (input, usage) = parse_tool_use(&canned, "t").unwrap();
        assert_eq!(input["ok"], true);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }
}
