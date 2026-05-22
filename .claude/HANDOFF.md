# HANDOFF — Claude Deck Stack Lift · Phase B

> **Resuming after a context clear.** Phase A (Scaffold) is complete and committed.
> This doc sets up **Phase B (feature parity)** for parallel multi-subagent
> execution, per the PROMPTER's directive. Delete this file when Phase B is done
> (WORKPLAN.md is the live tracker).

---

## 1. How to resume

1. Goal-driven `/goal` session — adopt the **ARCHITECT** identity (`~/.claude/agents/architect.md`). Framework state lives in the artifacts below; no need to re-run `/goal`.
2. Read, in order: `~/.claude/plans/zippy-bubbling-backus.md` (canonical 4-phase plan) · `PROJECT.md` (locked conventions + specialist roster) · `WORKPLAN.md` (live task tracker) · `~/Garden/admin/wiki/journals/2026May21_stack-lift-tauri-portable-pty-cm6-ai-sdk-z.md` (session journal — append to `## Log` as you work).
3. Resolve the §6 ops items with the PROMPTER, then execute §4 (Phase B parallel plan).

## 2. The goal

Lift Claude Deck onto **Tauri 2 · Rust · portable-pty · React 19 · TypeScript · xterm.js · CodeMirror 6 · Vercel AI SDK v6 · Tailwind v4 · shadcn/ui · Zustand** — a parallel `app/` tree on branch `lift/tauri-portable-pty-cm6-aisdk`, page-by-page, while `backend/` + `frontend/` keep shipping. Cutover only at end of Phase D.

## 3. State — Phase A complete (gate passed 6/6)

Branch `lift/tauri-portable-pty-cm6-aisdk`. Working tree clean. Feature commits:

| Task | Commit | Delivered |
|------|--------|-----------|
| A1 | `2e90d78` | `app/` skeleton tree + branch |
| A2a | `645d907` | `app/server` Cargo workspace — `server-core` lib + `server-bin` |
| A2b | `be40ee1` | `ServerConfig` refactor — `server-core` is config-in/Router-out (`app(config)->Router`); `server-bin` is the sole env reader |
| A3 | `c61c006` | `app/desktop` — Tauri 2 shell embedding `server-core` in-process |
| A4 | `a0ec741` | `app/web` — Vite 7 · React 19 · TS 5.9 strict · Tailwind v4 (`@theme inline`) · shadcn |
| A5 | `00ad63e` | Zustand stores `useProjectStore` + `useDashboardStore` (replace the legacy contexts) |
| A6 | `4240603` | `CodeEditor` — CodeMirror 6 wrapper (`app/web/src/components/shared/CodeEditor.tsx`) |
| A7 | `fdc9ce5` | AI proxy stub — `POST /api/v1/ai/chat`→501; AI SDK v6 client wired-disabled |
| A8 | `9c662a2` | Smoke harness — `core/tests/routes.rs` + `app/web/e2e/` Playwright nav |

`app/web` currently has only the scaffold: `MainLayout` shell (sidebar nav is a `task B1` placeholder), `HomePage`, `/dev/editor`, `/dev/ai`. Zero feature pages ported yet.

## 4. Phase B — parallel execution plan

Phase B ports the **20 legacy feature pages** onto the new stack. The PROMPTER wants maximum parallelism (multiple subagents at once). The hard constraint: **B1 is the foundation and cannot be parallelized — it runs first, solo.** Everything else depends on it; once B1 lands, the page ports + B5 run as a parallel wave.

### 4a. B1 — Foundation (FIRST, SOLO, engineer `david`) — this unlocks the parallelism

B1 must deliver the **complete shared base** so that page-port agents afterward have *zero shared-file dependencies*. B1 delivers:

1. **Theme fidelity** — verify/finish `app/web/src/index.css` against legacy `frontend/src/index.css` (the `@theme inline` structure is from A4 — confirm every token matches legacy).
2. **`MainLayout`** — faithful port of the legacy layout: sidebar, header, **dark-mode toggle** (port legacy `theme-toggle.tsx`). The sidebar nav MUST be **registry-driven** (renders from the registry in #6), never hardcoded page links.
3. **All shadcn/ui primitives** — port every legacy `frontend/src/components/ui/*` into `app/web/src/components/ui/`. A4 did only `button` + `card`; B1 ports the other 17: `alert-dialog, alert, badge, chart, checkbox, collapsible, dialog, input, label, progress, radio-group, scroll-area, select, switch, tabs, textarea, theme-toggle`.
4. **Shared components** — port `frontend/src/components/shared/MarkdownRenderer.tsx` and `MarkdownPreviewToggle.tsx` into `app/web/src/components/shared/`; wire the A6 `CodeEditor` into `MarkdownPreviewToggle`'s Edit tab (replacing the legacy textarea).
5. **Shared API + constants** — `app/web/src/lib/api.ts` (+ `buildEndpoint()`) and `lib/constants.ts` (`CLICKABLE_CARD`, `MODAL_SIZES`), ported from legacy `frontend/src/lib/`.
6. **★ The parallel-enabling deliverable — a glob-discovered feature-route registry.** Design: each feature will live in `app/web/src/features/<name>/` and drop a `route.tsx` exporting a descriptor `{ path, label, icon, order, Component }`. A registry collects them with Vite `import.meta.glob('./features/*/route.tsx', { eager: true })` — so **there is no central route file for page agents to edit**. `App.tsx` maps the registry → `<Route>`s inside `MainLayout`; the sidebar maps it (sorted by `order`) → nav links. B1 builds this registry, wires `App.tsx` + the sidebar to it, and migrates the existing HomePage / `/dev/editor` / `/dev/ai` onto the pattern as the proof it works.

B1 is the **Tailwind-3→4 visual-risk point** (plan risk table): port a couple of components side-by-side against legacy and **halt + surface if visual drift**. B1 gets a full, careful ARCHITECT review — especially deliverable #6, since all 19 page agents depend on the registry contract. **Do not start the §4b wave until B1 is verified and committed.**

### 4b. Parallel page-port wave (AFTER B1 — this is the parallelism)

19 feature pages. Because of the glob registry, each is a **self-contained `app/web/src/features/<name>/` directory** — a parallel agent touches ONLY its own dir (page + sub-components + types + its `route.tsx`) and nothing shared. No conflicts on `App.tsx`, `MainLayout`, or any registry file. Dispatch as **concurrent `david` subagents**.

Tiers (plan's low→high risk grouping; legacy source dir → new dir, same names):
- **B2 — stable / read-mostly (7):** `dashboard, config, presence, projects, plans, context, statusline`
- **B3 — markdown-editable, CM6 via `MarkdownPreviewToggle` (6):** `commands, agents, skills, memory, output-styles, hooks`
- **B4 — complex (6):** `mcp, plugins, permissions, sessions, backup, usage`

Recommended fan-out: group ~2–3 related pages per agent (≈7–9 concurrent `david` agents), or run tier-by-tier waves with each tier parallel internally — the ARCHITECT tunes the width against review bandwidth. The isolation contract (one feature dir per agent, glob registry) is what makes *any* width safe.

### 4c. B5 — cc-bridge v2 (PARALLEL track, `timothy` + `john`)

Independent subsystem — runs concurrently with the §4b wave. **Server:** `app/server/core/src/cc_bridge/{mod,pty,proto}.rs` on `portable-pty` with the new binary wire contract (see plan § cc-bridge v2). **Client:** `app/web/src/features/cc-bridge/` (xterm.js, binary-bytes contract) — fits the §4b feature-dir pattern, so it slots into the registry like any other page. The server side touches only `cc_bridge` files → no conflict with the page wave. `john` covers the same-origin / token security.

### 4d. Integration + per-page brief

With the glob registry, routes + nav auto-wire as feature dirs land — the "integration" is just: after the wave, verify the full sidebar renders and every route resolves, then run the §5 gate.

**Per-page engineer brief template** (fill `<name>` + tier specifics):
> ENGINEER for Phase B page port `<name>`, branch `lift/tauri-portable-pty-cm6-aisdk`. Port legacy `frontend/src/features/<name>/` faithfully into a NEW self-contained `app/web/src/features/<name>/` directory. Consume the B1 shared base only: shadcn primitives from `@/components/ui/*`, `@/components/shared/{MarkdownRenderer,MarkdownPreviewToggle}`, `@/lib/api.ts`, the theme. Export a `route.tsx` descriptor `{ path, label, icon, order, Component }` per the B1 registry contract. **Touch ONLY `app/web/src/features/<name>/`** — never `App.tsx`, `MainLayout`, the registry, or another feature's dir (the glob registry auto-wires you). TS strict, no `any`. B3 pages: use `MarkdownPreviewToggle` (CM6) for editable markdown. Acceptance: `npm run build` + `lint` green; the page renders without console errors; an `e2e/<name>.spec.ts` nav test passes. Do NOT commit — ARCHITECT reviews + commits.

## 5. Phase B gate — 7 feature-parity validators (all required to merge)

1. Every `frontend/src/features/*` page has an `app/web/src/features/*` counterpart rendering without console errors against the same backend.
2. Playwright nav smoke green for all 20 pages (extend `app/web/e2e/`).
3. cc-bridge connects a real shell, echoes input, resizes, exits cleanly; same-origin enforced.
4. Mobile-responsive layout (iPhone width 375) verified on ≥1 ported page.
5. LAN access still works (a device on the tailnet hits `server-bin` over LAN).
6. No schema changes to `claude_registry.db`.
7. Old `backend/` + `frontend/` still build from `main`.

## 6. Pending ops state — RESOLVE WITH THE PROMPTER FIRST

- **`com.moonexpr.claude-deck` launchd service is STOPPED.** The PROMPTER authorised stopping it for the Phase-A dev test (it's a `KeepAlive` service running the old `backend` on port 8000). The new `server-bin` took port 8000. **Decision needed:** leave it stopped through Phase B dev (new `server-bin` keeps :8000 — simplest) or reload it (then Phase B's dev `server-bin` needs another port). Reload command: `launchctl bootstrap gui/501 ~/Library/LaunchAgents/com.moonexpr.claude-deck.plist`. The PROMPTER's deployed instance is offline until this is reloaded — flag it.
- **Dev servers** were running pre-clear: `server-bin` on `:8000`, `app/web` Vite on `:5174`. They may not survive the context clear — check `lsof -nP -iTCP:8000 -iTCP:5174 -sTCP:LISTEN` and restart if wanted: `cd app/server && ./target/debug/server-bin` ; `npm --prefix app/web run dev`.

## 7. Session gotchas (carry forward)

- **Commits need the gpg-loopback wrapper:** `git -c gpg.program=gpg-loopback commit -m "..."` — a bare `git commit` hangs on pinentry. Put `git add` in the same Bash call as the commit.
- **`rm` is blocked** by security policy — use `git clean -f <paths>` for untracked files; `git restore <path>` to revert tracked ones.
- **`curl`/`wget` are blocked** — probe HTTP via Node `fetch` inside `ctx_execute`.
- **think-in-code gate** active — route large/recursive shell output through `ctx_batch_execute`/`ctx_execute`; no bare `cat`.
- **Bash cwd persists** between calls — a `cd .../app/server` earlier makes later relative paths resolve there; prefix git/ls/test commands with an explicit `cd` to the repo root.
- **ARCHITECT/ENGINEER discipline:** ARCHITECT writes no implementation code — engineers implement, ARCHITECT verifies (inspect the diff + re-run every acceptance check) then commits. In Phase A, 4 of 8 tasks needed exactly one corrective round caught at review — expect the same vigilance.
- Conventional commits, no co-author tag, scope = subsystem. Commit each task as its own unit; WORKPLAN/journal bumps as a follow-up `chore`. Per-page commits in Phase B: `feat(web): port <name> page (B2/3/4)`.
- The session journal lives in the `~/Garden` repo (separate from this repo) — edit it; its commit is the Garden repo's concern.
- Untracked noise to ignore, never `git add`: `backend/build_errors.txt`, `.claude/scheduled_tasks.lock`.
- Plan freezes versions through Phases A–C (realign in D5) — do not bump `VERSION`/`health()` version strings mid-lift.
