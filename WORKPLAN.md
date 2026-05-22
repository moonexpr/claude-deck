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
- ✅ **B2** Stable pages — `dashboard, config, presence, projects, plans, context, statusline`
- ✅ **B3** Markdown-editable pages, CM6 — `commands, agents, skills, memory, output-styles, hooks`
- ✅ **B4** Complex pages — `mcp, plugins, permissions, sessions, backup, usage`
- ✅ **B5** cc-bridge v2 — portable-pty server + xterm client (`3f43e59`); cross-feature import resolved, orphan-PTY kill + absent-Origin rejection hardening, 6 functional tests (`ccec058`). `john` security review done — structural auth items deferred to a logged security pass (`docs/requests/INBOX.md`).

**Wave outcome:** the parallel worktree wave collapsed (worktree isolation flaky + an account session limit cut 7/10 agents) — recovered in-place via a main-tree recovery wave. Commits: `5a58e81` (wave pile preserved) · `5d567a0` (worktree-branch collect) · `a55b170` (19 page ports) · `3f43e59` (cc-bridge) · `3e60e2c` (e2e green). **Playwright sweep 49/49 green.**

**Phase B gate — ✅ 7 / 7 — PHASE B COMPLETE:** ✅(1) all 20 pages render w/o console errors vs server-bin · ✅(2) Playwright sweep 61/61 green · ✅(3) cc-bridge functional + same-origin enforced (6 Rust tests) · ✅(4) mobile-responsive iPhone-375 (`mobile.spec.ts`) · ✅(5) LAN — `server-bin` reachable + serving the UI over the tailnet, PROMPTER-confirmed · ✅(6) no DB schema change · ✅(7) legacy `backend/`+`frontend/` untouched (`git diff main` empty).

Post-Phase-B fixes from the PROMPTER's hands-on tailnet review: `server-bin` `FRONTEND_DIST` (blank page) · mobile drawer rebuilt — position, scroll, single X, bordered links (`248a93c`, `c3ab7f9`) · SPA deep-link 404 → 200 (`db59a14`). Deferred to `docs/requests/INBOX.md`: post-lift design pass (UI density + UX flows), Hooks-badge redesign, CM6 for config renders, cc-bridge auth hardening.

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
