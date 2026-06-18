//! Headless Claude Code inference for the Insight Platform.
//!
//! Instead of calling the Anthropic API directly (which bills per-token against
//! `ANTHROPIC_API_KEY`), we shell out to the `claude` CLI in print mode
//! (`claude -p … --output-format json`). This uses the logged-in Claude Code
//! **subscription**. We remove `ANTHROPIC_API_KEY` from the child environment
//! so `claude` authenticates with the subscription rather than API credits —
//! otherwise a set key routes back to the API (and e.g. "Credit balance is too
//! low") instead of the subscription.

use crate::services::ai::anthropic::Usage;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;

/// Slice a JSON object out of free-form model text (tolerates code fences/prose).
pub(crate) fn extract_json(text: &str) -> Result<Value> {
    let start = text.find('{');
    let end = text.rfind('}');
    if let (Some(s), Some(e)) = (start, end)
        && e > s
        && let Ok(v) = serde_json::from_str::<Value>(&text[s..=e])
    {
        return Ok(v);
    }
    Err(anyhow!("could not parse a JSON object from claude output"))
}

/// Parse a `claude --output-format json` envelope into (schema object, usage),
/// or an error when the CLI reported `is_error`.
pub(crate) fn parse_envelope(envelope: &Value) -> Result<(Value, Usage)> {
    if envelope.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        let msg = envelope
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(anyhow!("claude error: {msg}"));
    }
    let result = envelope
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("claude envelope missing string `result`"))?;
    let parsed = extract_json(result)?;
    let usage = Usage {
        input_tokens: envelope
            .pointer("/usage/input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: envelope
            .pointer("/usage/output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    Ok((parsed, usage))
}

/// Run a single-shot headless `claude` generation and return the parsed JSON
/// object the model emitted (per `system`'s instructions) plus token usage.
pub async fn run_structured(
    claude_bin: &str,
    model: Option<&str>,
    system: &str,
    user_prompt: &str,
) -> Result<(Value, Usage)> {
    let mut cmd = tokio::process::Command::new(claude_bin);
    cmd.arg("-p")
        .arg(user_prompt)
        .arg("--output-format")
        .arg("json")
        .arg("--system-prompt")
        .arg(system)
        // Pure text generation — no agentic tools.
        .arg("--disallowed-tools")
        .arg("Bash Edit Write Read WebFetch WebSearch")
        // Force subscription auth, not API credits.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(m) = model {
        cmd.arg("--model").arg(m);
    }

    let out = cmd.output().await.with_context(|| {
        format!("failed to spawn `{claude_bin}` (is Claude Code installed and on PATH?)")
    })?;

    if out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!(
            "`{claude_bin}` produced no output (status {}): {}",
            out.status,
            err.trim()
        ));
    }

    let envelope: Value = serde_json::from_slice(&out.stdout)
        .context("claude did not return JSON (expected --output-format json)")?;
    parse_envelope(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_handles_code_fence_and_prose() {
        let v = extract_json("sure:\n```json\n{\"a\":1}\n```").unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn parse_envelope_success_extracts_result_and_usage() {
        let env = serde_json::json!({
            "is_error": false,
            "result": "{\"summary\":\"hi\",\"follow_ups\":[]}",
            "usage": { "input_tokens": 100, "output_tokens": 7 }
        });
        let (v, u) = parse_envelope(&env).unwrap();
        assert_eq!(v["summary"], "hi");
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 7);
    }

    #[test]
    fn parse_envelope_surfaces_is_error() {
        let env = serde_json::json!({ "is_error": true, "result": "Credit balance is too low" });
        let e = parse_envelope(&env).unwrap_err().to_string();
        assert!(e.contains("Credit balance"), "got: {e}");
    }
}
