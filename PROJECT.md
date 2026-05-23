# PROJECT.md

Project-specific conventions for Claude Deck. Architect-maintained. Reference these from any goal-driven or orchestrated session.

> Operational rules live in `~/.claude/CLAUDE.md`. This file captures repo-local decisions only.

---

## Active session

**Goal:** Lift Claude Deck onto Tauri 2 · Rust · portable-pty · React 19 · TS · xterm.js · CodeMirror 6 · Vercel AI SDK v6 · Tailwind v4 · shadcn/ui · Zustand.

**Canonical plan:** `~/.claude/plans/zippy-bubbling-backus.md` (approved 2026-05-21).
**Branch:** `lift/tauri-portable-pty-cm6-aisdk` (created in A1).
**Workplan tracker:** `WORKPLAN.md` (repo root, session-scoped; deleted in Build phase per framework).

---

## Testing philosophy

**Floor: smoke harness only.** No unit tests are part of this lift.

- Rust: `app/server/core/tests/routes.rs` integration tests assert every `/api/v1/*` route family returns the expected status (200/4xx). Stubs returning empty JSON pass as long as status is correct.
- Frontend: Playwright nav smoke at `app/web/e2e/*.spec.ts`. One test per ported page asserting it mounts without console errors.
- A stronger floor (unit tests on Zustand stores, AI proxy key handling, cc-bridge same-origin, PTY protocol parsing) is **post-merge work**, not gated by this lift.

Rationale: the existing tree ships zero tests today; introducing a heavy test suite alongside a stack lift compounds risk. Smoke gives us page-by-page regression detection without bloating Phase A.

---

## Git / PR conventions

- **Conventional commits** (`type(scope): subject`). Types: `feat`, `fix`, `chore`, `port`, `docs`, `refactor`, `test`, `perf`. Scope is the subsystem (`server-core`, `web`, `desktop`, `cc-bridge`, `ai-proxy`, etc.).
- **Commit often during Dev.** Squash into per-phase logical units at Build (D1). Preserve meaningful boundaries within a phase (don't collapse Phase B's 5 sub-stages into one commit).
- **No co-author tag.** No "Generated with Claude Code" or "Co-Authored-By" lines in commit messages. (Per global `memory/general.md`.)
- **Commit signing.** Per `~/.claude/rules/git-commit-signing.md`.
- **Single PR.** `lift/tauri-portable-pty-cm6-aisdk` → `main` at the end of Phase D. Old `backend/` + `frontend/` rename to `legacy/backend/` + `legacy/frontend/` in the same PR; retire in a separate cleanup PR.
- **No force-pushes** to `lift/*` after others may have pulled it. Use `--force-with-lease` only if explicitly requested.

---

## Platform / framework freezes

| Layer | Choice | Notes |
|---|---|---|
| Desktop shell | **Tauri 2** | In-process embed of `server-core`. macOS-first signing/notarize, Linux AppImage, Windows deferred. |
| Language (server) | Rust 2024 | `cargo build` from `app/server/`. |
| HTTP framework | axum 0.8 | Keep current crate set; no migration to actix/poem. |
| PTY | `portable-pty` | Hosts child process. Drops tmux-capture model. |
| Database | SQLite via sqlx 0.8 | `aiosqlite://` URL strings are dead — sqlx parses `sqlite:` only. |
| Migrations | `sqlx-migrate` | Introduced in Phase C (C3) for `chat_conversations`. |
| Frontend framework | React 19.2 | Keep current. |
| Build tool | Vite 7 | Keep current. |
| Language (frontend) | TypeScript 5.9 strict | `noUnusedLocals`, `noUnusedParameters` on. |
| CSS framework | **Tailwind v4** | `@theme` directive in `src/index.css`. No `tailwind.config.ts`. |
| UI primitives | **shadcn/ui (Tailwind-4 templates)** | Re-derive CSS vars from legacy `frontend/src/index.css`. |
| State | **Zustand** | Replaces `ProjectContext` + `DashboardContext` only. Per-feature state stays local. |
| Editor | **CodeMirror 6** | Single `CodeEditor` component, language: markdown / json / shell. |
| AI SDK | **Vercel AI SDK v6** | Client uses `baseURL: '/api/v1/ai'`. No provider keys in browser. |
| AI provider | Anthropic only (Phase C) | OpenAI / others are post-merge work. |
| Terminal | xterm.js (kept) | Updated client to new binary-bytes wire contract in B5. |

### What is **out of scope** for this lift

- Filling the ~15 backend stub routes — that is a parallel workstream, not gated by this lift.
- Replacing `react-router-dom` with TanStack Router or similar — keep what works.
- Migrating off SQLite.
- Adding additional AI providers beyond Anthropic.
- Refactoring `lib/api.ts` to per-feature `api.ts` files (convention preserved).
- Windows signing/installer.
- Replacing the existing `backend_python/` directory (legacy reference; cleanup is a separate PR).
- Fixing the Dockerfile (Python residue, `aiosqlite://`, missing Rust stage) unless it blocks cutover.

---

## Specialist roster (active for this goal)

| Phase task | Genre / Principal |
|---|---|
| A2 server-core split | `moses` |
| A3 Tauri embed | `timothy` |
| A4 Tailwind 4 + shadcn | `david` |
| A6 CodeMirror adapter | `david` |
| A8 smoke harness | `luke` |
| B1–B4 page ports | `david` |
| B5 portable-pty | `timothy` + `john` |
| C1–C2 AI proxy + keyring | `isaiah` + `john` |
| C3 chat panel | `isaiah` + `david` |
| C4 cc-bridge AI augmentation | `isaiah` + `john` |
| D3 Tauri sign/notarize | `jacob` |
| D4 docs | `mark` |
