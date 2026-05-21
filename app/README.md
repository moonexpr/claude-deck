# app/ — Stack Lift (parallel tree)

This is the **new-stack** rebuild of Claude Deck, developed on branch
`lift/tauri-portable-pty-cm6-aisdk` alongside the still-shipping `backend/` +
`frontend/` trees. The old trees stay authoritative until this tree reaches
feature parity and is cut over (Phase D).

**Plan:** `~/.claude/plans/zippy-bubbling-backus.md` · **Conventions:** `../PROJECT.md` · **Tracker:** `../WORKPLAN.md`

## Layout

| Dir | Stack | Role |
|-----|-------|------|
| `server/` | Rust 2024 · axum 0.8 · sqlx · portable-pty · Cargo workspace | `core` library + `bin` binary. The HTTP/WS backend. Embeddable as a Tauri sidecar **and** runnable standalone for LAN/mobile. |
| `desktop/` | Tauri 2 · Rust | Desktop shell. Embeds `server/core` in-process; webview hosts `web/`. |
| `web/` | React 19 · Vite 7 · TS · Tailwind 4 · shadcn/ui · Zustand · CodeMirror 6 · xterm.js · Vercel AI SDK v6 | The UI. One bundle, served both into the Tauri webview and by `server/bin` to browsers. |

## Why a parallel tree

The lift changes the deploy shape (desktop + sidecar), the PTY model
(portable-pty hosting, not tmux-capture polling), the CSS framework (Tailwind
3→4), and adds AI surfaces. Rebuilding in place would leave `main` unstable for
the whole migration. The parallel tree lets the old app keep shipping while
each page is ported and verified one at a time.

## Status

Phase A (scaffold) in progress. See `../WORKPLAN.md`.
