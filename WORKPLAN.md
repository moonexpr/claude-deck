# WORKPLAN — Stack Lift

Session workplan. Canonical plan: `~/.claude/plans/zippy-bubbling-backus.md`. Conventions: `PROJECT.md`.
Deleted at Build phase (D).

Status legend: ⬜ todo · 🔵 in progress · ✅ done · ⏸ blocked

---

## Phase A — Scaffold

- ✅ **A1** Branch `lift/tauri-portable-pty-cm6-aisdk` + `app/` skeleton tree — `2e90d78`
- ✅ **A2a** `app/server` Cargo workspace — copy `backend/src` into `core` lib + thin `bin` — `645d907`
- ✅ **A2b** Introduce `ServerConfig`; eliminate env reads from handlers; `core::app(config)` entry point — `be40ee1`
- ✅ **A3** Tauri 2 bootstrap at `app/desktop` — in-process embed, bootstrap page hits `/api/v1/status` — `c61c006`
- ✅ **A4** `app/web` Vite/React/TS scaffold — Tailwind 4 `@theme`, shadcn/ui, MainLayout shell — `a0ec741`
- ✅ **A5** Zustand stores `useProjectStore`, `useDashboardStore`; MainLayout consumes — `00ad63e`
- ✅ **A6** `CodeEditor` component (CM6 markdown mode) + demo route — `4240603`
- ✅ **A7** `/api/v1/ai/chat` returns 501 + key-source diagnostics; AI SDK client wired, disabled — `fdc9ce5`
- ✅ **A8** Smoke harness — Rust route tests + Playwright nav config + one passing test — `9c662a2`

**Phase A gate:** all 6 scaffolded validators in the plan pass.

## Phase B — Feature parity

- ⬜ **B1** Theme + layout port (MainLayout, sidebar, header, dark-mode, shadcn primitives)
- ⬜ **B2** Stable pages: Dashboard, Config, Presence, Projects, Plans, Context, Status line
- ⬜ **B3** Markdown-editable pages w/ CM6: Commands, Agents, Skills, Memory, Output Styles, Hooks
- ⬜ **B4** Complex pages: MCP, Plugins, Permissions, Sessions, Backup, Usage
- ⬜ **B5** cc-bridge v2 — portable-pty host + new wire contract + xterm client rewrite

**Phase B gate:** all 7 feature-parity validators pass.

## Phase C — New AI surfaces

- ⬜ **C1** Server AI proxy `/api/v1/ai/chat` + `/suggest`
- ⬜ **C2** Tauri keyring → `ServerConfig::anthropic_api_key`
- ⬜ **C3** Chat panel feature + `sqlx-migrate` + `chat_conversations` table
- ⬜ **C4** cc-bridge AI augmentation (confirm-before-exec gate)
- ⬜ **C5** AI-assisted config editing — Agents page template
- ⬜ **C6** CM6 for JSON surfaces: MCP, Permissions

**Phase C gate:** all 5 new-surface validators pass.

## Phase D — Build / cutover

- ⬜ **D1** Squash dev commits into per-phase units
- ⬜ **D2** `cargo fmt` / `clippy` / `eslint` / `tsc -b`
- ⬜ **D3** Tauri sign + notarize (macOS), AppImage (Linux)
- ⬜ **D4** README + `app/README.md`
- ⬜ **D5** Extend `bump-version.sh` for `tauri.conf.json`
- ⬜ **D6** Dockerfile fix-or-file
- ⬜ **D7** Open PR; rename old trees to `legacy/`
