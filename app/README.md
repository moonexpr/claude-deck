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

## Anthropic key resolution

The AI proxy (`/api/v1/ai/chat`, `/api/v1/ai/suggest`) only fires when `ServerConfig.anthropic_api_key` is `Some`. Resolution order:

1. **Tauri builds** — `app/desktop/src-tauri/src/keychain.rs` reads the OS keychain entry `(service: "claude-deck", account: "anthropic_api_key")`. On macOS:
   ```bash
   security add-generic-password -s claude-deck -a anthropic_api_key -w 'sk-ant-…'
   ```
2. **Standalone `server-bin`** — reads `ANTHROPIC_API_KEY` env.
3. **Neither** — `/api/v1/ai/chat` returns 503 with `{ anthropic_key_configured: false }`. The web UI surfaces a non-blocking banner pointing the user at the keychain / env paths.

The key never leaves the server: only the `x-api-key` header to Anthropic. Browser DevTools Network shows no key on any client request.

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
