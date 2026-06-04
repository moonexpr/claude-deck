#!/usr/bin/env python3
"""hf-chat — generic chat-completion against the Hugging Face Inference router.

OpenAI-compatible call to https://router.huggingface.co/v1/chat/completions
using $HF_TOKEN. Unlike the adversarial-testing helper, this applies NO framing
— the prompt you pass is the prompt the model sees. Use it to offload a
self-contained task to a non-Anthropic model (DeepSeek, Kimi, Qwen, …) whose
output you then verify by execution.

Usage:
  hf-chat.py --user @task.md [--system @sys.md] [--model deepseek] [--json]
  echo "question" | hf-chat.py --model kimi

--user / --system accept inline text or @path (file) or - (stdin).
--model accepts a short alias (deepseek|kimi|qwen|llama) or a full HF model id.
Prints the assistant message content to stdout; model + usage to stderr.
Soft-fails across the alias's fallback list; exit 1 if all fail, 2 on bad input.
"""
import argparse, json, os, sys, urllib.request, urllib.error

URL = "https://router.huggingface.co/v1/chat/completions"

ALIASES = {
    "deepseek": ["deepseek-ai/DeepSeek-V3-0324", "deepseek-ai/DeepSeek-V3-0324:novita"],
    "kimi": ["moonshotai/Kimi-K2-Instruct", "moonshotai/Kimi-K2-Instruct:novita"],
    "qwen": ["Qwen/Qwen2.5-72B-Instruct"],
    "llama": ["meta-llama/Llama-3.3-70B-Instruct"],
}


def _read(spec):
    if spec is None:
        return None
    if spec == "-":
        return sys.stdin.read()
    if spec.startswith("@"):
        return open(spec[1:], encoding="utf-8").read()
    return spec


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--user", required=True, help="user prompt: inline | @file | -")
    ap.add_argument("--system", help="system prompt: inline | @file | -")
    ap.add_argument("--model", default="deepseek", help="alias or full HF model id")
    ap.add_argument("--max-tokens", type=int, default=4000)
    ap.add_argument("--temperature", type=float, default=0.2)
    ap.add_argument("--json", action="store_true", help="request JSON object output")
    args = ap.parse_args()

    token = os.environ.get("HF_TOKEN")
    if not token:
        print("NO_HF_TOKEN: export HF_TOKEN", file=sys.stderr)
        return 2

    user = (_read(args.user) or "").strip()
    if not user:
        print("EMPTY_USER_PROMPT", file=sys.stderr)
        return 2
    system = _read(args.system)

    messages = []
    if system:
        messages.append({"role": "system", "content": system.strip()})
    messages.append({"role": "user", "content": user})

    models = ALIASES.get(args.model, [args.model])
    body = {
        "messages": messages,
        "max_tokens": args.max_tokens,
        "temperature": args.temperature,
    }
    if args.json:
        body["response_format"] = {"type": "json_object"}

    last_err = None
    for model in models:
        body["model"] = model
        req = urllib.request.Request(
            URL, data=json.dumps(body).encode(),
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                out = json.load(resp)
            content = out["choices"][0]["message"]["content"]
            usage = out.get("usage", {})
            print(f"=== MODEL: {model} | usage: {usage} ===", file=sys.stderr)
            print(content)
            return 0
        except urllib.error.HTTPError as e:
            last_err = f"{model}: HTTP {e.code} {e.read()[:300]!r}"
            print(last_err, file=sys.stderr)
        except Exception as e:  # noqa: BLE001
            last_err = f"{model}: {type(e).__name__} {e}"
            print(last_err, file=sys.stderr)

    print(f"ALL_MODELS_FAILED: {last_err}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
