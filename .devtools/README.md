# .devtools — repo-local development tooling

Helpers used during development that aren't part of the shipped app and aren't
shell entrypoints under `scripts/`. Each tool is self-describing; run with
`--help`.

## bin/

| Tool | What it does | Run |
|------|--------------|-----|
| `hf-chat.py` | Offload a self-contained task to a non-Anthropic model (DeepSeek, Kimi, Qwen, Llama) via the Hugging Face Inference router. OpenAI-compatible, uses `$HF_TOKEN`. No prompt framing — what you pass is what the model sees. Output is meant to be **verified by execution** (run the tests), never trusted on the model's say-so. | `.devtools/bin/hf-chat.py --user @task.md --model deepseek` |

### hf-chat.py

```bash
# Offload an impl/edit task to DeepSeek, capture the output, then VERIFY with tests.
.devtools/bin/hf-chat.py --user @task.md --model deepseek
echo "explain this regex" | .devtools/bin/hf-chat.py --model kimi

# --user / --system accept inline text, @path (file), or - (stdin)
# --model accepts an alias (deepseek|kimi|qwen|llama) or a full HF model id
# --json requests a JSON object response
```

Requires `HF_TOKEN` in the environment. Soft-fails across the alias's fallback
model list; exits non-zero only if every model errors.
