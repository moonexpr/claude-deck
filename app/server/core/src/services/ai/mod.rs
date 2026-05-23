/// AI service layer: Anthropic API client and Vercel Data Stream proxy.
///
/// Module layout:
///   anthropic — raw Anthropic Messages API client (streaming + non-streaming)
///   proxy     — transform Anthropic SSE events → Vercel Data Stream v1 frames

pub mod anthropic;
pub mod proxy;

/// A single chat message in the request body.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}
