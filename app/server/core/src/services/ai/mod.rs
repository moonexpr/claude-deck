/// AI service layer: Anthropic API client and Vercel Data Stream proxy.
///
/// Module layout:
///   anthropic    — raw Anthropic Messages API client (streaming + non-streaming)
///   proxy        — transform Anthropic SSE events → Vercel Data Stream v1 frames
///   key_provider — credential abstraction (D7); ai.rs asks here per request
pub mod anthropic;
pub mod key_provider;
pub mod proxy;

/// The role of a chat message.
///
/// Serialized as lowercase strings on the wire (`"system"`, `"user"`,
/// `"assistant"`), matching both the Vercel AI SDK and Anthropic wire formats.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// A single chat message in the request body.
///
/// The `role` field is an enum to prevent invalid role strings from entering
/// the system. Wire format is unchanged: `"system"`, `"user"`, `"assistant"`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_role_roundtrip_user() {
        let msg = Message { role: MessageRole::User, content: "hi".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"user\""), "user serializes to lowercase: {}", json);
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, MessageRole::User);
    }

    #[test]
    fn message_role_roundtrip_assistant() {
        let msg = Message { role: MessageRole::Assistant, content: "ok".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"assistant\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, MessageRole::Assistant);
    }

    #[test]
    fn message_role_roundtrip_system() {
        let msg = Message { role: MessageRole::System, content: "You are helpful.".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"system\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, MessageRole::System);
    }
}
