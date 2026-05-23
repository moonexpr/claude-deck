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
///
/// Token counts are accumulated across `UsageUpdate` events emitted from
/// `message_start` (carries `input_tokens`) and `message_delta` (carries
/// cumulative `output_tokens`). On `MessageStop`, the accumulated counts are
/// written into the `d:` frame so it always reflects real usage rather than 0.
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};

use super::anthropic::AnthropicEvent;

/// Transform a stream of `AnthropicEvent`s into Vercel Data Stream v1 `Bytes`.
///
/// For each `TextDelta`, emit a `0:` frame.
/// For `UsageUpdate`, fold the counts into the running accumulator (no output).
/// For `MessageStop`, emit the `d:` finalization frame with accumulated counts.
/// `Other` events are silently dropped.
///
/// The returned stream ends after the `d:` frame.
pub fn anthropic_sse_to_data_stream(
    event_stream: impl Stream<Item = anyhow::Result<AnthropicEvent>>,
) -> impl Stream<Item = Bytes> {
    // Stateful scan: carry (input_tokens, output_tokens) forward across the
    // stream. `scan` emits `Option<Bytes>` — None for non-emitting events.
    let scanned = event_stream.scan(
        (0u64, 0u64), // (accumulated_input, accumulated_output)
        |acc, result| {
            let frame: Option<Option<Bytes>> = match result {
                Err(_) => Some(None),
                Ok(AnthropicEvent::TextDelta { text }) => {
                    let json_text = serde_json::to_string(&text).unwrap_or_else(|_| {
                        format!("\"{}\"", text.replace('"', "\\\""))
                    });
                    Some(Some(Bytes::from(format!("0:{}\n", json_text))))
                }
                Ok(AnthropicEvent::UsageUpdate { input_tokens, output_tokens }) => {
                    // Fold: take the max of each field across all UsageUpdate events.
                    // message_start carries input_tokens; message_delta carries
                    // cumulative output_tokens. Both may be present.
                    if input_tokens > 0 {
                        acc.0 = input_tokens;
                    }
                    if output_tokens > 0 {
                        acc.1 = output_tokens;
                    }
                    Some(None) // no output frame for usage updates
                }
                Ok(AnthropicEvent::MessageStop) => {
                    let (input, output) = *acc;
                    let frame = Bytes::from(format!(
                        "d:{{\"finishReason\":\"stop\",\"usage\":{{\"promptTokens\":{},\"completionTokens\":{}}}}}\n",
                        input, output
                    ));
                    Some(Some(frame))
                }
                Ok(AnthropicEvent::Other) => Some(None),
            };
            std::future::ready(frame)
        },
    );
    // Flatten: drop the None entries, yield the Some(Bytes) values.
    scanned.filter_map(std::future::ready)
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
pub fn finish_frame(input_tokens: u64, output_tokens: u64) -> Bytes {
    Bytes::from(format!(
        "d:{{\"finishReason\":\"stop\",\"usage\":{{\"promptTokens\":{},\"completionTokens\":{}}}}}\n",
        input_tokens, output_tokens
    ))
}
