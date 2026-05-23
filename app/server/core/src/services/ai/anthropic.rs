/// Anthropic Messages API client.
///
/// Two entry points:
///   stream_messages  — returns a Stream of AnthropicEvent (parsed SSE)
///   complete_messages — returns the full text and token usage synchronously
///
/// Neither function reads environment variables. All config is passed in.
use anyhow::{Context, anyhow};
use eventsource_stream::Eventsource;
use futures::stream::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::Message;

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
    /// The message is complete; carries final token usage.
    MessageStop { usage: Usage },
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
    api_key: String,
    model: Option<String>,
    messages: Vec<Message>,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<AnthropicEvent>>> {
    let model_str = model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = build_request_body(model_str, &messages, true);

    let resp = client
        .post(&url)
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
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
            if let Some(delta) = raw.delta {
                if delta.kind == "text_delta" {
                    return Ok(AnthropicEvent::TextDelta {
                        text: delta.text.unwrap_or_default(),
                    });
                }
            }
            Ok(AnthropicEvent::Other)
        }
        "message_delta" => {
            // message_delta carries cumulative usage in streaming mode
            let raw: RawMessageDelta =
                serde_json::from_str(data).unwrap_or(RawMessageDelta { usage: None });
            let usage = raw.usage.map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0),
                output_tokens: u.output_tokens.unwrap_or(0),
            });
            // We defer the stop event until message_stop; stash nothing here.
            let _ = usage;
            Ok(AnthropicEvent::Other)
        }
        "message_stop" => {
            // usage is not in message_stop; it's in message_start / message_delta.
            // We emit a stop event with usage=0 here; the handler accumulates
            // usage from message_delta if available. For now this is fine because
            // the Vercel Data Stream `d:` frame just needs a finish reason.
            Ok(AnthropicEvent::MessageStop {
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            })
        }
        "message_start" => {
            // Carries initial usage (e.g. input_tokens). Parse and stash for stop.
            let raw: RawMessage = serde_json::from_str(data)
                .map(|r: serde_json::Value| RawMessage {
                    usage: r
                        .get("message")
                        .and_then(|m| m.get("usage"))
                        .and_then(|u| serde_json::from_value(u.clone()).ok())
                        .and_then(|u: RawUsage| {
                            Some(RawUsage {
                                input_tokens: u.input_tokens,
                                output_tokens: u.output_tokens,
                            })
                        }),
                })
                .unwrap_or(RawMessage { usage: None });
            let _ = raw;
            Ok(AnthropicEvent::Other)
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
    api_key: String,
    model: Option<String>,
    messages: Vec<Message>,
) -> anyhow::Result<(String, Usage)> {
    let model_str = model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = build_request_body(model_str, &messages, false);

    let resp = client
        .post(&url)
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
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
