# HANDOFF — Claude Deck Stack Lift (resume at A8)

> **Resuming — read this first.** Mid-flight `/goal` session, handed off because
> the A8 engineer hit a session limit (account limit resets ~19:10
> America/Los_Angeles, 2026-05-21). Phase A is 7/8 done. Finish A8, verify the
> Phase A gate, then delete this file (WORKPLAN.md is the live tracker).

---

## 1. How to resume

1. Goal-driven session — adopt the **ARCHITECT** identity (`~/.claude/agents/architect.md`); the framework state lives in the artifacts below, no need to re-run `/goal`.
2. Read: `~/.claude/plans/zippy-bubbling-backus.md` (canonical plan) · `PROJECT.md` (conventions, specialist roster) · `WORKPLAN.md` (live task tracker) · `~/Garden/admin/wiki/journals/2026May21_stack-lift-tauri-portable-pty-cm6-ai-sdk-z.md` (session journal — append to `## Log`).
3. Execute §4 (finish A8). Then §5 (Phase A gate + checkpoint).

## 2. The goal

Lift Claude Deck onto **Tauri 2 · Rust · portable-pty · React 19 · TypeScript · xterm.js · CodeMirror 6 · Vercel AI SDK v6 · Tailwind v4 · shadcn/ui · Zustand**, built in a parallel `app/` tree on branch `lift/tauri-portable-pty-cm6-aisdk`, page-by-page, while `backend/` + `frontend/` keep shipping. Cutover only at end of Phase D.

## 3. Progress — Phase A (Scaffold), 7 of 8 tasks committed

| Task | Commit | State |
|------|--------|-------|
| A1 scaffold `app/` tree | `2e90d78` | ✅ |
| A2a `app/server` Cargo workspace | `645d907` | ✅ |
| A2b `ServerConfig` refactor (config-in/Router-out) | `be40ee1` | ✅ |
| A3 Tauri 2 bootstrap (`app/desktop`, in-process embed) | `c61c006` | ✅ |
| A4 `app/web` Vite/React/Tailwind4/shadcn scaffold | `a0ec741` | ✅ |
| A5 Zustand stores (`useProjectStore`,`useDashboardStore`) | `00ad63e` | ✅ |
| A6 `CodeEditor` (CodeMirror 6) + demo route | `4240603` | ✅ |
| A7 AI proxy stub (`/api/v1/ai/chat`→501) + AI SDK client | `fdc9ce5` | ✅ |
| **A8 smoke harness** | — | ⬜ **PARTIAL — finish per §4** |

ARCHITECT/ENGINEER split is in force: a genre engineer implements, ARCHITECT verifies the acceptance criteria (inspect diff + run checks) then commits. A4/A5/A6 each needed one ARCHITECT-caught corrective round.

## 4. Finish A8 — the smoke harness

A8's engineer (`luke`) was cut off by a session limit. Current uncommitted state in the working tree:

- **Part 1 (Rust route smoke) — written but DOES NOT COMPILE.** `app/server/core/tests/routes.rs` exists: one `#[tokio::test] all_route_families_mounted_and_respond` driving every `/api/v1/*` family via `tower::ServiceExt::oneshot`. It fails to compile — `core/Cargo.toml` has **no `[dev-dependencies]`**, so `use tower::...` and `use http_body_util::...` are unresolved (13 errors, all cascading from those two).
- **Part 2 (Playwright) — NOT STARTED.** No `app/web/playwright.config.ts`, no `app/web/e2e/`, no `@playwright/test` dep.

**To finish A8 — dispatch a `luke` engineer (subagent_type `luke`) with this brief:**

````
You are the ENGINEER finishing task A8 (smoke harness) of the Claude Deck stack-lift, branch `lift/tauri-portable-pty-cm6-aisdk`, repo `/Users/jc/Garden/external/claude-deck/`. A prior pass wrote `app/server/core/tests/routes.rs` but was cut off. Finish A8.

Part 1 — make the Rust route smoke compile and pass:
1. Add a `[dev-dependencies]` section to `app/server/core/Cargo.toml` with `tower = { version = "0.5", features = ["util"] }` and `http-body-util = "0.1"` (the versions `app/desktop/src-tauri/Cargo.toml` already uses). No new runtime deps.
2. `cd app/server && cargo test -p server-core` — make `tests/routes.rs` compile and pass. If a route assertion fails because the test guessed a wrong path/method (e.g. it references `/api/v1/status-info/` — verify that route actually exists in `core/src/api/v1/`; the real status route is `GET /api/v1/status`), fix the TEST to hit the real endpoint with the real expected status. Do NOT modify route handlers to make a test pass — a genuine 500 is a real bug; stop and report it.
3. Every `/api/v1/*` family + `/health` must be covered and green.

Part 2 — Playwright nav smoke in `app/web`:
4. Install `@playwright/test` as an `app/web` devDependency; `npx playwright install chromium`.
5. Add `app/web/playwright.config.ts` (a `webServer` block auto-starting the app so `npx playwright test` is self-contained; `testDir: 'e2e'`; chromium project).
6. Add `app/web/e2e/nav.spec.ts` — one test: goto `/`, assert the `MainLayout` shell renders and there are no console errors. One test only.
7. Add `"test:e2e": "playwright test"` to `app/web/package.json`. Ensure `npm run build` (`tsc -b`) and `npm run lint` still pass with the e2e files present (scope tsconfig/eslint so e2e `.ts` files don't break them). Playwright artifact dirs are already gitignored.

Acceptance — ALL must pass: (1) `cargo test -p server-core` green, all families covered; (2) `cargo build` green; (3) `@playwright/test` installed; (4) `npx playwright test` in `app/web` green; (5) `npm run build` + `npm run lint` green; (6) `git status --porcelain -- backend/ frontend/ app/desktop/` clean (untracked `backend/build_errors.txt` is pre-existing). Report per-criterion with evidence. Do NOT commit — the ARCHITECT reviews and commits.
````

When it returns: ARCHITECT verifies all 6 criteria (run `cargo test`, `cargo build`, `npx playwright test`, `npm build`+`lint`; inspect the diff — esp. that no route handler was altered to pass a test), then commits as `test(app): A8 smoke harness — Rust route tests + Playwright nav`, updates WORKPLAN (A8 ✅ + SHA) and the journal, and **deletes this HANDOFF.md**.

## 5. After A8 — Phase A gate, then checkpoint

Phase A gate = the 6 validators in the plan: `cargo build -p server-core -p server-bin` ✓ · `cargo test -p server-core` ✓ · `npm --prefix app/web run build` ✓ · `npx playwright test` ✓ · `cargo tauri dev` shows the bootstrap page (A3 — user-confirmed already) · old `backend/`+`frontend/` still build on `main`. Verify these, report **Phase A complete** to the PROMPTER, and checkpoint before Phase B (the 20-page parity port, B1–B5).

## 6. Session gotchas (carry forward)

- **Commits need the gpg-loopback wrapper:** `git -c gpg.program=gpg-loopback commit -m "..."` — a bare `git commit` hangs on pinentry. Run `git add` in the same Bash call as the commit.
- **`rm` is blocked** by security policy — use `git clean -f <paths>` to remove untracked files.
- **`curl`/`wget` are blocked** — probe HTTP via Node `fetch` in a `ctx_execute`/script.
- **think-in-code gate** is active — route large/recursive shell output through `ctx_batch_execute`/`ctx_execute`, not raw Bash; no bare `cat`.
- Conventional commits, no co-author tag. Commit each task as its own unit; tracker bumps as a follow-up `chore`.
- The session journal lives in the `~/Garden` repo (separate from this repo) — edit it; its commit is the Garden repo's concern.
- Untracked noise to ignore, never `git add`: `backend/build_errors.txt`, `.claude/scheduled_tasks.lock`.
