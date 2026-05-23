# app/ — Stack Lift

The next-generation Claude Deck build, on branch `lift/tauri-portable-pty-cm6-aisdk`. Phases A / B / C complete; Phase D (build + cutover) in progress.

**Plan:** `~/.claude/plans/zippy-bubbling-backus.md` · **Phase-C detail:** `../.claude/plans/phase-c-ai-surfaces.md` · **Tracker:** `../WORKPLAN.md`

## Layout

| Dir | Stack | Role |
|-----|-------|------|
| `server/` | Rust 2024 · axum 0.8 · sqlx (with `sqlx::migrate!`) · portable-pty · Cargo workspace | `core` library + `bin` binary. HTTP/WS backend with the same `/api/v1/*` surface. Embeddable as a Tauri sidecar **and** runnable standalone for LAN/mobile. |
| `desktop/` | Tauri 2 · Rust | Desktop shell. Embeds `server/core` in-process. Reads the Anthropic API key from the OS keychain (`keyring` crate). |
| `web/` | React 19 · Vite 7 · TS · Tailwind 4 · shadcn/ui · Zustand · CodeMirror 6 · xterm.js · Vercel AI SDK v6 | The UI. One bundle, served both into the Tauri webview and by `server/bin` to browsers. |

## Quick dev

```bash
# Standalone server + browser UI (LAN/mobile path)
cd app/server && cargo run -p server-bin                 # binds 0.0.0.0:8000
cd app/web && npm install && npm run dev                 # Vite dev server at :5173

# Desktop (Tauri webview embedding the in-process server)
cd app/desktop && cargo tauri dev                        # ephemeral 127.0.0.1:<port>
```

The Vite dev server proxies `/api` to `127.0.0.1:8000`. For LAN access:

```bash
cd app/server && HOST=0.0.0.0 cargo run -p server-bin
# Reach the UI at http://<host>:8000 (server-bin also serves the built web bundle when FRONTEND_DIST is set)
```

## Credential sources

The AI proxy (`/api/v1/ai/chat`, `/api/v1/ai/suggest`) resolves a
credential per request via `ServerConfig.key_source: KeySource`. Two
paths are supported:

### Path A — Anthropic API key (`sk-ant-…`)

Pay-per-token billing on the Anthropic Console. Sent to upstream as
`x-api-key`. This is the original path.

1. **Tauri builds** — `app/desktop/src-tauri/src/keychain.rs` reads
   the OS keychain entry `(service: "claude-deck", account:
   "anthropic_api_key")`. On macOS:
   ```bash
   security add-generic-password -s claude-deck -a anthropic_api_key -w 'sk-ant-…'
   ```
2. **Standalone `server-bin`** — reads `ANTHROPIC_API_KEY` env.

### Path B — Claude Code OAuth (experimental, closes #4)

Reuses the user's existing Claude Code login (Pro / Max subscription)
instead of provisioning a separate API key. The
[`claudecode_ext`](https://github.com/moonexpr/claudecode_ext) framework
runs a local TLS-MITM proxy that Claude Code's outbound HTTPS traffic
flows through; the observed OAuth bearer is reused by the Deck's AI
proxy and sent to Anthropic as `Authorization: Bearer …`.

1. **Tauri builds** — auto-detected. If no `(claude-deck,
   anthropic_api_key)` entry exists in the keychain **and**
   `Claude Code-credentials` does (probed via `security
   find-generic-password` without reading the value, so no Touch ID
   prompt), the Tauri build spawns the framework automatically.
2. **Standalone `server-bin`** — opt-in via env var:
   ```bash
   CLAUDECODE_EXT_KEY_SOURCE=oauth cargo run -p server-bin
   ```
   Optional overrides: `CLAUDECODE_EXT_BIND`, `CLAUDECODE_EXT_CA_DIR`,
   `CLAUDECODE_EXT_DISCOVERY`.

**Launching Claude Code through the proxy.** The framework only
observes traffic that flows through it. After the Deck is running with
the OAuth source, launch `claude` via the included `claude_ext` shim
binary instead of the bare `claude` binary:

```bash
# Build the shim once:
cd ~/Garden/app/claudecode_ext && cargo build --release -p claudecode_ext_shim
# Then use it in place of `claude`:
./target/release/claude_ext --print "hello"
# OR symlink into PATH:
ln -s "$PWD/target/release/claude_ext" ~/.local/bin/
```

The shim reads `~/.claudecode_ext/proxy.sock` (the discovery file the
framework writes at startup), exports `HTTPS_PROXY` +
`NODE_EXTRA_CA_CERTS`, and `execvp`s `claude`. Bun honors both env
vars, so the user-facing Claude Code experience is unchanged — only
the network path changes.

> **Experimental.** Claude Code's auth flow is an internal Anthropic
> implementation detail and can change without notice. Path B may
> break with any Claude Code update. Reusing the OAuth credential in
> a third-party process may also be against Anthropic's terms — the
> "right" long-term fix is for Anthropic to expose a public OAuth
> client registration. Use Path A in production.

### No source configured

If neither path resolves, `/api/v1/ai/chat` returns 503 with:

```json
{
  "status": "unavailable",
  "anthropic_key_configured": false,
  "key_source": null
}
```

When a path *is* configured but currently unable to produce a
credential (e.g. OAuth selected but Claude Code hasn't run through
`claude_ext` recently enough), `key_source` reports the label
(`"api_key"` or `"oauth"`) so the UI can suggest the right next step.

### Privacy posture

For Path A: the key never leaves the server; only the `x-api-key`
header to Anthropic. Browser DevTools Network shows no key on any
client request.

For Path B: the framework is **observe-only** — bytes flow through
unchanged, no traffic modification. The bearer captured from Claude
Code is held in memory by the framework and never logged. The CA root
is stored under `~/.claudecode_ext/ca/` (key file 0600) and trusted
process-scoped via `NODE_EXTRA_CA_CERTS`; no system-trust-store
modification.

## Migrations

`sqlx::migrate!("./migrations")` runs every boot from `app/server/core/src/lib.rs`. Migrations live at `app/server/core/migrations/`. The first migration (`0001_chat_conversations.sql`) lands in Phase C.

Schema changes may be destructive (DROP/recreate over careful ALTER) per project policy — Claude Deck's DB content is small and easy to recreate.

## What's new vs `backend/` + `frontend/`

- **Tauri desktop** — single binary; OS keychain integration; in-process axum + webview.
- **portable-pty** — replaces tmux-capture polling for CC Bridge. Direct PTY child hosting via WebSocket binary frames.
- **CodeMirror 6** — markdown surfaces (commands, agents, skills, memory, output-styles, hooks) plus read-only JSON (config viewer).
- **AI surfaces** — `useChat` chat panel, `<execute>`-tag confirm-before-exec on CC Bridge, AI-suggest on Agents (template for Commands / Skills / Memory).
- **Tailwind 4** — `@theme` directive, no `tailwind.config.ts`; shadcn templates ported.
- **Zustand** — `useProjectStore` + `useDashboardStore` replace the prior Contexts.

## Cutover

The legacy `backend/` and `frontend/` trees are **untouched** by this branch (`git diff main -- backend/ frontend/` is empty). They keep shipping until the cleanup PR renames them to `legacy/`.

## Out of scope (tracked in `docs/requests/INBOX.md`)

- Post-lift design pass (UI density + UX flows)
- cc-bridge auth hardening (token endpoint, port-exempt origin, CORS scope, token-store eviction, session caps, weak `rand_token` fallback) — gated by the deployment (CF Access + tailnet).
- Hooks event-type badge redesign.
- Dockerfile rewrite (current Dockerfile is Python-era — see GitHub issue).
- Tauri macOS notarization + Linux AppImage packaging (needs Apple Developer credentials).
