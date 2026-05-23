/// Frame types for the portable-pty binary wire contract.
///
/// TEXT frames (JSON, client→server or server→client):
///   Open    – first message after WS upgrade, client→server
///   Resize  – either direction
///   Signal  – client→server
///   Exit    – server→client, terminal frame
///
/// BINARY frames:
///   Stdin   – client→server raw bytes → pty master write
///   Stdout  – server→client raw bytes from pty master read
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Client → server frames
// ---------------------------------------------------------------------------

/// The first text message from the client after WS upgrade.
/// Instructs the server to spawn a child via portable-pty.
#[derive(Debug, Deserialize)]
pub struct OpenFrame {
    /// Must equal "open".
    pub kind: String,
    /// Command + arguments, e.g. ["claude", "--", "..."].
    pub cmd: Vec<String>,
    /// Working directory for the child process.
    pub cwd: String,
    /// Extra environment variables to inject.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Initial terminal column count.
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Initial terminal row count.
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_cols() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}

/// Resize request (either direction).
#[derive(Debug, Deserialize, Serialize)]
pub struct ResizeFrame {
    /// Must equal "resize".
    pub kind: String,
    pub cols: u16,
    pub rows: u16,
}

/// Signal request (client→server).
#[derive(Debug, Deserialize)]
pub struct SignalFrame {
    /// Must equal "signal".
    pub kind: String,
    /// One of "INT", "TERM", "KILL".
    pub sig: String,
}

/// Discriminated union for all incoming text frames.
#[derive(Debug)]
pub enum ClientTextFrame {
    Open(OpenFrame),
    Resize(ResizeFrame),
    Signal(SignalFrame),
    Unknown,
}

/// Parse an incoming text WebSocket message into a `ClientTextFrame`.
pub fn parse_client_text(text: &str) -> ClientTextFrame {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return ClientTextFrame::Unknown;
    };
    match value.get("kind").and_then(|v| v.as_str()) {
        Some("open") => match serde_json::from_value::<OpenFrame>(value) {
            Ok(f) => ClientTextFrame::Open(f),
            Err(_) => ClientTextFrame::Unknown,
        },
        Some("resize") => match serde_json::from_value::<ResizeFrame>(value) {
            Ok(f) => ClientTextFrame::Resize(f),
            Err(_) => ClientTextFrame::Unknown,
        },
        Some("signal") => match serde_json::from_value::<SignalFrame>(value) {
            Ok(f) => ClientTextFrame::Signal(f),
            Err(_) => ClientTextFrame::Unknown,
        },
        _ => ClientTextFrame::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Server → client frames
// ---------------------------------------------------------------------------

/// Sent when the child process exits. Terminal frame — socket closes after this.
#[derive(Debug, Serialize)]
pub struct ExitFrame {
    pub kind: &'static str,
    pub code: i32,
}

impl ExitFrame {
    pub fn new(code: i32) -> Self {
        ExitFrame { kind: "exit", code }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"kind":"exit","code":-1}"#.to_string())
    }
}
