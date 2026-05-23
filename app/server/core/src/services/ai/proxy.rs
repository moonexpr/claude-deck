/// Vercel AI SDK Data Stream Protocol v1 encoder.
///
/// Frame syntax (each frame ends with `\n`):
///   0:"<json-encoded text chunk>"\n   — text delta
///   d:{...}\n                         — final frame (finishReason + usage)
///
/// Reference: https://sdk.vercel.ai/docs/ai-sdk-ui/stream-protocol
///
/// `anthropic_sse_to_data_stream` consumes an `AnthropicEvent` stream and
/// emits `Bytes` values that axum can forward to the client chunk by chunk.

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};

use super::anthropic::{AnthropicEvent, Usage};

/// Transform a stream of `AnthropicEvent`s into Vercel Data Stream v1 `Bytes`.
///
/// For each `TextDelta`, emit a `0:` frame.
/// For `MessageStop`, emit the `d:` finalization frame.
/// `Other` events are silently dropped.
///
/// The returned stream ends after the `d:` frame.
pub fn anthropic_sse_to_data_stream(
    event_stream: impl Stream<Item = anyhow::Result<AnthropicEvent>>,
) -> impl Stream<Item = Bytes> {
    event_stream.filter_map(|result| {
        let frame: Option<Bytes> = match result {
            Err(_) => None,
            Ok(AnthropicEvent::TextDelta { text }) => {
                // JSON-encode the text string so embedded quotes/newlines are safe.
                let json_text = serde_json::to_string(&text).unwrap_or_else(|_| {
                    format!("\"{}\"", text.replace('"', "\\\""))
                });
                Some(Bytes::from(format!("0:{}\n", json_text)))
            }
            Ok(AnthropicEvent::MessageStop { usage }) => {
                // Emit the Vercel Data Stream finalization frame.
                // Usage values of 0 are fine; the client uses them for display only.
                Some(Bytes::from(format!(
                    "d:{{\"finishReason\":\"stop\",\"usage\":{{\"promptTokens\":{},\"completionTokens\":{}}}}}\n",
                    usage.input_tokens, usage.output_tokens
                )))
            }
            Ok(AnthropicEvent::Other) => None,
        };
        std::future::ready(frame)
    })
}

/// Encode a single text chunk as a Vercel Data Stream `0:` frame.
///
/// Exposed for unit testing.
pub fn text_delta_frame(text: &str) -> Bytes {
    let json_text = serde_json::to_string(text).unwrap_or_else(|_| format!("\"{}\"", text));
    Bytes::from(format!("0:{}\n", json_text))
}

/// Encode a final `d:` frame for the given usage values.
///
/// Exposed for unit testing.
pub fn finish_frame(usage: &Usage) -> Bytes {
    Bytes::from(format!(
        "d:{{\"finishReason\":\"stop\",\"usage\":{{\"promptTokens\":{},\"completionTokens\":{}}}}}\n",
        usage.input_tokens, usage.output_tokens
    ))
}
