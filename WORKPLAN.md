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

**Phase A gate:** ✅ **PASSED 6/6** — workspace build · `cargo test` route smoke · web build · Playwright nav · desktop build+embed · legacy trees byte-identical to `main`.

## Phase B — Feature parity  ·  parallel execution — see `.claude/HANDOFF.md` §4

**Execution model:** **B1 runs FIRST and SOLO** — it is the foundation and unlocks parallelism by establishing a glob-discovered feature-route registry (page agents then never touch shared files). After B1 is verified+committed, the page ports run as a **parallel wave** of `david` subagents — each owns one isolated `app/web/src/features/<name>/` dir — alongside B5 on its own track.

- ✅ **B1** Foundation (solo, `david`) — theme fidelity vs legacy · `MainLayout` + registry-driven sidebar + dark-mode toggle · the 17 remaining shadcn primitives · `MarkdownRenderer` + `MarkdownPreviewToggle` (CM6) · `lib/api.ts` + `constants.ts` · **the `import.meta.glob` feature-route registry** (`route` | `routes[]`, discriminated `FeatureRoute`) · sonner `<Toaster>` · `ProjectSwitcher` in `components/layout/`. Theme token diff clean (no Tailwind 3→4 drift). — `459cad7`
- ✅ **B1+** Shared-base completion — 18 type files → `app/web/src/types/` (`@/types/` kept shared: 5 type files are cross-feature) · `RefreshButton` (all 19 pages) + `JsonViewer` shared components. — `d3d9d9c`
- 🔵 **B2** Stable pages — parallel after B1: `dashboard, config, presence, projects, plans, context, statusline`
- 🔵 **B3** Markdown-editable pages, CM6 — parallel after B1: `commands, agents, skills, memory, output-styles, hooks`
- 🔵 **B4** Complex pages — parallel after B1: `mcp, plugins, permissions, sessions, backup, usage`
- 🔵 **B5** cc-bridge v2 — portable-pty host + new wire contract + xterm client — parallel track (`timothy` + `john`)

**Wave dispatched** (10 agents, isolated worktrees, background): B2a `[dashboard,config,statusline]` · B2b `[presence,projects,plans]` · B2c `[context]` · B3a `[commands,agents]` · B3b `[skills,output-styles]` · B3c `[memory,hooks]` · B4a `[mcp]` · B4b `[plugins,permissions,backup]` · B4c `[sessions,usage]` · B5 `[cc-bridge]`. ARCHITECT reviews each, collects disjoint feature dirs into the main tree, commits per page; then the consolidated build + Playwright sweep.

**Phase B gate:** all 7 feature-parity validators pass (see `.claude/HANDOFF.md` §5).

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
