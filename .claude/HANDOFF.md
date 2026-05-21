# HANDOFF — Claude Deck Stack Lift

> **Resuming session — read this first.** This is a mid-flight `/goal` session
> handed off for a context reset. Everything you need to continue is here or
> linked from here. After you have resumed and completed A2b, delete this file
> (it is superseded by `WORKPLAN.md` as the live tracker).

---

## 1. How to resume

1. This is a **goal-driven session**. Re-activate the phase-mode framework — invoke `/goal` with the goal text in §2, or if `/goal` is unavailable, adopt the framework manually: assume the **ARCHITECT** identity (`~/.claude/agents/architect.md`).
2. **Current state: Dev Phase, Design Mode**, mid-task **A2b**.
3. Read, in order:
   - `~/.claude/plans/zippy-bubbling-backus.md` — the approved canonical plan (4 phases, validators, risks).
   - `PROJECT.md` (repo root) — locked conventions (testing, git, platform freezes).
   - `WORKPLAN.md` (repo root) — live task tracker with status + commit SHAs.
   - `~/Garden/admin/wiki/journals/2026May21_stack-lift-tauri-portable-pty-cm6-ai-sdk-z.md` — session journal. **Append to its `## Log` as you work; fill `## Close` at session end.**
4. Then execute §5 (the A2b engineer brief).

---

## 2. The goal

Lift Claude Deck's frontend + backend onto a new stack — **Tauri 2 · Rust · portable-pty · React 19 · TypeScript · xterm.js · CodeMirror 6 · Vercel AI SDK v6 · Tailwind v4 · shadcn/ui · Zustand** — built in a parallel `app/` tree on branch `lift/tauri-portable-pty-cm6-aisdk`, page-by-page, while the existing `backend/` + `frontend/` keep shipping. Cutover only at the end of Phase D.

Four locked decisions (do not relitigate): Tauri + sidecar server (mobile/LAN preserved) · AI SDK in all three roles (chat panel, cc-bridge augment, AI config editing) · parallel tree · Zustand replaces `ProjectContext`+`DashboardContext` only. Plus: portable-pty *hosts* the child process (tmux-attach dropped); CM6 markdown-first; smoke-tests-only floor; tree at `app/{desktop,server,web}`. Full rationale in the plan.

---

## 3. Progress so far

Branch `lift/tauri-portable-pty-cm6-aisdk`, two commits past `main` (`80d353d`):

| Task | Commit | State |
|------|--------|-------|
| **A1** scaffold `app/` tree + `.gitignore` hardening + `PROJECT.md` + `WORKPLAN.md` | `2e90d78` | ✅ verified |
| **A2a** copy `backend/src` → `app/server` Cargo workspace (`server-core` lib + `server-bin`) | `645d907` | ✅ verified — `cargo build` ok, `/api/v1/status`→200, `backend/` untouched |
| **A2b** `ServerConfig` refactor | — | ⬜ **NEXT — brief in §5** |

**Decomposition note:** the plan's single task "A2" was split by the ARCHITECT into **A2a** (mechanical move — done) + **A2b** (the refactor — next) for one-cycle sizing. This is a legitimate design-mode decomposition; keep the split.

**Current `app/server/` layout** (post-A2a): Cargo workspace, `members = ["core","bin"]`. `core/` = package `server-core`, lib crate, holds a verbatim copy of old `backend/src/**`; `core/src/lib.rs` currently exposes `pub async fn run() -> anyhow::Result<()>` (the old `main()` body). `bin/` = package `server-bin`, `src/main.rs` just calls `server_core::run().await`. `backend/` is the OLD tree — **read-only**, still ships, never edit it.

---

## 4. Session gotchas (carry these forward)

- **Commits need the gpg-loopback wrapper.** A bare `git commit` hangs on pinentry (no TTY). Use: `git -c gpg.program=gpg-loopback commit -m "..."`. Also: the commit-sign PreToolUse hook blocks the *whole* Bash call, so `git add` must be re-run in the same command as the wrapped commit (a blocked command runs nothing).
- **`curl`/`wget` are blocked** by the context-mode gate. To hit an HTTP endpoint, use `mcp__plugin_context-mode_context-mode__ctx_execute` with `language: "javascript"` and Node's `fetch`.
- **think-in-code gate** is active: route large/recursive shell output through `ctx_batch_execute` / `ctx_execute`, not raw Bash.
- **`SendMessage` is not available** here — you cannot resume a prior subagent. Spawn fresh `Agent` calls with self-contained briefs.
- **Specialist engineers**: spawn the genre principal by `subagent_type`. A2b's engineer is `moses` (Backend). Roster for later tasks is in `PROJECT.md`.
- Commit style: conventional commits, no co-author tag, scope = subsystem (`server`, `web`, `desktop`, `cc-bridge`, …). Commit each task as its own logical unit; ARCHITECT reviews engineer output (trust-but-verify: inspect the actual diff + run the acceptance check) before committing.
- Untracked noise in the tree (`backend/build_errors.txt`, `backend/target/*`, db `-shm`/`-wal`) is pre-existing — ignore it; never `git add` it.

---

## 5. Immediate next action — dispatch A2b

Spawn an `Agent` with `subagent_type: "moses"` and the following **verbatim** prompt. When it returns, ARCHITECT verifies the four acceptance criteria (inspect the diff; run the build + a `ctx_execute` fetch of `/api/v1/status`; confirm `backend/` untouched), then commits as `port(server): ...` and advances to A3.

````
You are the ENGINEER for task A2b of a stack-lift on the Claude Deck repo (`/Users/jc/Garden/external/claude-deck/`). Branch: `lift/tauri-portable-pty-cm6-aisdk` (already checked out — work only here).

## Current state (from the just-completed task A2a)

`app/server/` is a Cargo workspace with two crates:
- `app/server/core/` — package `server-core`, lib crate. Contains a verbatim copy of the old `backend/src/**`. Its `src/lib.rs` currently exposes `pub async fn run() -> anyhow::Result<()>` — that fn is the old `main()` body: it inits tracing, calls `dotenvy::dotenv()`, builds the SQLite pool, runs schema bootstrap, builds the CORS layer + axum Router, then binds `HOST:PORT` and serves.
- `app/server/bin/` — package `server-bin`, binary crate. `src/main.rs` is just `#[tokio::main] async fn main() { server_core::run().await }`.

`backend/` is the OLD tree — it must stay byte-identical (read-only; copy/reference only, never edit).

`cd app/server && cargo build` works today. `cargo run -p server-bin` boots and `GET /api/v1/status` returns HTTP 200.

## Why this task exists

The desktop app (Tauri, a later task) will embed `server-core` in-process. An embedded library must not reach into process-global `std::env` or `current_dir()` for per-instance configuration — the embedder has to be able to supply it. So `server-core` must become **config-in, Router-out**, and `server-bin` becomes the only place that reads the environment.

## Your task — A2b

### 1. Define the config struct in `server-core` (this is the fixed contract — do not rename these fields)

```rust
pub struct ServerConfig {
    pub db_url: String,                       // was DATABASE_URL
    pub projects_dir: std::path::PathBuf,     // was PROJECTS_DIR
    pub frontend_dist_path: Option<std::path::PathBuf>, // None => do not mount static frontend serving
    pub presence_public_url: Option<String>,  // was PRESENCE_PUBLIC_URL
    pub anthropic_api_key: Option<String>,    // unused until a later phase — carry it through now
    pub enable_external_tools: bool,          // gates PATH-dependent external-tool discovery
}
```
You MAY add extra fields if you genuinely need one (e.g. to replace a `current_dir()` fallback base path) — if you do, report it. You may NOT remove or rename the six above.

### 2. Replace `run()` with the library entry point

```rust
pub async fn app(config: ServerConfig) -> anyhow::Result<axum::Router>;
```

`app()` does ONLY: build the SQLite pool (from `config.db_url`), run schema bootstrap, build CORS, build and return the `Router`. It must NOT init tracing, must NOT call `dotenvy`, must NOT bind a socket. Delete `run()`.

### 3. Move env/process concerns into `server-bin`

`server-bin/src/main.rs` becomes the only env reader: init tracing, `dotenvy::dotenv()`, read env vars, construct a `ServerConfig`, call `server_core::app(config).await?`, read `HOST`/`PORT`, bind, and serve. Env → config mapping for the bin:
- `DATABASE_URL` → `db_url` (same default as today: `sqlite:claude_registry.db?mode=rwc`)
- `PROJECTS_DIR` → `projects_dir` (same default as today — `~/.claude/projects`)
- `PRESENCE_PUBLIC_URL` → `presence_public_url` (`Option`, no default)
- `ANTHROPIC_API_KEY` → `anthropic_api_key` (`Option`, no default)
- `FRONTEND_DIST` → `frontend_dist_path` if set, else `None` (the new tree has no frontend build yet — `None` is the normal case)
- `enable_external_tools` → default `true` (preserves today's behavior); optionally honor an `ENABLE_EXTERNAL_TOOLS` env var
- `HOST`/`PORT` stay bin-only — used for binding, never put in `ServerConfig`.

### 4. Eliminate these specific env reads from `server-core` (the offenders)

- `core/src/api/v1/mod.rs` — reads `PROJECTS_DIR` when constructing the router/`ApiState`. Use `config.projects_dir` instead. Thread the config (or the needed fields) into `ApiState` so handlers can read them.
- `core/src/api/v1/presence.rs` — reads `PRESENCE_PUBLIC_URL` inside a handler. Use `config.presence_public_url` via `ApiState`.
- `core/src/api/v1/memory.rs` and `core/src/api/v1/plans.rs` — call `std::env::current_dir()` inside handlers. Investigate what base path that provides; replace it with an explicit value carried in `ServerConfig`/`ApiState` (capture it once in the bin if a CWD-equivalent is genuinely needed). No `current_dir()` inside `core`.
- `core/src/lib.rs` — the frontend dist path is derived from `env!("CARGO_MANIFEST_DIR")`. Replace with `config.frontend_dist_path`: when `Some`, mount `ServeDir` as today; when `None`, skip static-frontend serving entirely (API routes still mount).
- `core/src/api/v1/{agents,plugins,mcp,cli}.rs` — these read `PATH` to discover external tools. Reading `PATH` itself at request time is acceptable (it is a legitimate process-global). But each of these PATH-dependent discovery paths must be gated by `config.enable_external_tools`: when `false`, the route degrades gracefully (return the same empty/default result a missing tool would yield) instead of scanning `PATH`.

`server-core` must end up with **zero** reads of `DATABASE_URL`, `PROJECTS_DIR`, `PRESENCE_PUBLIC_URL`, `current_dir()`, and `CARGO_MANIFEST_DIR`. `PATH` reads may remain but only behind the `enable_external_tools` gate.

## Constraints

- `backend/` is read-only. Verify untouched at the end.
- Behavior with default env must be identical to today: same routes, same schema bootstrap, same responses. The only intended behavior change is that static-frontend serving is now off by default (because `FRONTEND_DIST` is unset and the new tree has no frontend yet) — that is correct and expected.
- No new dependencies. No `portable-pty`, no cleanup of unrelated code, no "while I'm here."
- Keep `ApiState` as the state-threading mechanism; extend it, don't replace the pattern.

## Acceptance criteria — done when ALL pass

1. `cd app/server && cargo build` succeeds (pre-existing non-snake-case warnings in `hooks.rs` are fine; no new errors).
2. `grep -rnE 'std::env::(var|var_os|current_dir)|env!\("CARGO_MANIFEST_DIR"\)|"PROJECTS_DIR"|"PRESENCE_PUBLIC_URL"|"DATABASE_URL"' app/server/core/src` returns NOTHING except, at most, `PATH` reads inside `agents/plugins/mcp/cli` that are gated by `enable_external_tools`. Report the exact grep output.
3. `cd app/server && cargo run -p server-bin` boots; `GET /api/v1/status` → 200; `GET /api/v1/projects` and a real presence GET route still respond as before.
4. `git status --porcelain -- backend/` shows no modifications to `backend/`.

## Report back (under 300 words)

The final `ServerConfig` definition (including any field you added, with why), the `app()` signature, the new `server-bin/src/main.rs` in full, the grep output for criterion 2, and per-criterion confirmation. Do NOT commit — the ARCHITECT reviews and commits. If an interface question blocks you, stop and report rather than guessing.
````

---

## 6. After A2b — what comes next

Phase A continues: **A3** Tauri 2 bootstrap (`timothy`) → **A4** Vite/React/Tailwind4/shadcn scaffold (`david`) → **A5** Zustand stores → **A6** CodeMirror adapter → **A7** AI proxy stub → **A8** smoke harness (`luke`). Phase A gate = the 6 scaffolded validators in the plan. Then Phase B (page-by-page parity), C (AI surfaces), D (build/cutover). Full detail in `~/.claude/plans/zippy-bubbling-backus.md` and `WORKPLAN.md`.
