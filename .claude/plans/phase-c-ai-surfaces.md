# Phase C — New AI surfaces

Branch: `lift/tauri-portable-pty-cm6-aisdk` · Parent plan: `~/.claude/plans/zippy-bubbling-backus.md` §Phase C.

Phase B PASSED 7/7. Entry state at `e8733e7`. This plan refines C1–C6 into a concrete execution order with locked judgment calls, parallelism boundaries, and an explicit gate.

---

## Locked judgment calls (made by ARCHITECT — confirm or override before Work)

1. **Provider transport.** No official Rust SDK for Anthropic. Use `reqwest` + `eventsource-stream` to call `POST https://api.anthropic.com/v1/messages` with `stream: true`. Hand-rolled, ~200 LOC. Avoids dragging in an unmaintained third-party crate.
2. **Wire format to the browser.** AI SDK v6's `useChat` expects the **Vercel Data Stream Protocol** (line-oriented `0:"..."` / `2:[]` / `d:{}` frames), *not* Anthropic SSE. Server transforms Anthropic `content_block_delta` events into Data-Stream frames. Pass-through Anthropic SSE would force the client to ship a custom parser — rejected.
3. **Persistence shape (C3).** **One table, `chat_conversations`**, with `messages JSONB NOT NULL` storing the full transcript as a JSON array. Simpler migration, no foreign-key dance, no second `chat_messages` table. Future-extensible: if we later need per-message indexing, we add a derived `chat_messages` table in a second migration without breaking C3.
4. **Migration tooling.** `sqlx::migrate!()` macro pointing at `app/server/core/migrations/`. Migration `0001_chat_conversations.sql`. Runs on every `core::app(config)` boot — idempotent because `_sqlx_migrations` tracks applied versions. Empty migrations dir today = idempotent no-op on existing DBs.
5. **Tauri keyring (C2).** `keyring` crate (cross-platform: macOS Keychain · Linux secret-service · Windows Credential Manager). Read in `app/desktop/src-tauri/src/main.rs` *before* `core::app(config)` is invoked, passed via `ServerConfig.anthropic_api_key`. No `tauri-plugin-*` dependency.
6. **C4 confirm-before-exec UX.** Inline diff panel above the terminal. Model-suggested command renders as preview text with explicit **[Send] / [Edit] / [Discard]** buttons. PTY stdin write is reachable only via the **Send** action. **Never** auto-routes. (Acceptance: a stub model emitting `<execute>rm -rf /</execute>` must produce a visible preview and zero PTY bytes until the human clicks Send.)
7. **C4 vs cc-bridge auth INBOX.** Out of scope. The confirm gate is a UX boundary, not auth. The INBOX hardening (unauthenticated token endpoint, port-exempt origin check, permissive CORS, unbounded token store, session caps, weak token fallback) remains deferred. C4's plan doc and code comments will say so explicitly.
8. **Dev-ai demo route.** The leftover `app/web/src/features/dev-ai/AiDemoPage.tsx` (A7 stub, never registered) is repurposed as the C1 streaming smoke route during dev; deleted in the Phase C closing commit.

---

## Execution order

```
C1 ──► C2 ──► C3 ──┬──► C4 (cc-bridge AI augmentation)
                   ├──► C5 (Agents AI-suggest template)
                   └──► C6 (CM6 for MCP + Permissions JSON)
```

**Rationale.** C1 unlocks every AI consumer. C2 is independent of C1 but tiny (~50 LOC) — runs serially right after to keep the diff log clean. C3 introduces `sqlx::migrate!()` plus the `migrations/` directory — must land *before* the parallel wave so the migration scaffolding exists. C4/C5/C6 then run as a parallel wave because they touch disjoint feature dirs:
- C4 → `app/web/src/features/cc-bridge/` + new `app/server/core/src/services/ai_proxy_*` helpers
- C5 → `app/web/src/features/agents/`
- C6 → `app/web/src/features/{mcp,permissions}/`

No shared file overlap → safe parallel wave with `isaiah` (C4 logic), `david` (C5 UI), `david` (C6 UI). C2 keyring is a `timothy` (Rust) task.

---

## Task breakdown

### C1 — Server AI proxy (`isaiah` + `john`)

**Targets:**
- `app/server/core/src/api/v1/ai.rs` — replace 501 stub with two streaming handlers.
- `app/server/core/src/services/ai/{mod,anthropic,proxy}.rs` — new service module: Anthropic client + SSE → Data-Stream transform.
- `app/server/core/tests/ai_proxy.rs` — three integration tests: (a) no-key → 503 with diagnostic; (b) bad upstream → 502; (c) happy path → stream of `0:"..."` frames.
- Crate add: `eventsource-stream = "0.2"`.

**Endpoints:**
- `POST /api/v1/ai/chat` — streaming chat. Body: `{ messages: [{role, content}], model?: string }`. Streams Data-Stream Protocol over `text/plain; charset=utf-8`.
- `POST /api/v1/ai/suggest` — single-shot. Same body, returns `{ text: string, usage: {...} }`.

**Same-origin check.** Reuse `cc_bridge::is_same_origin`. Document the inherited port-exempt looseness in the route doc comment — it's the same boundary as cc-bridge, no new attack surface.

**Validators:**
- `cargo test -p server-core --test ai_proxy` green (3/3).
- DevTools Network: no `x-api-key` on `/api/v1/ai/*` requests (key never leaves server).
- 503 body carries `anthropic_key_configured: false` so the disabled-state UI works.

---

### C2 — Tauri keyring (`timothy`)

**Targets:**
- `app/desktop/src-tauri/Cargo.toml` — add `keyring = "3"`.
- `app/desktop/src-tauri/src/main.rs` — read entry `("claude-deck", "anthropic_api_key")` before spawning `core::app(config)`. On `NoEntry` / `NoBackend` errors, leave key as `None` (server returns 503 with diagnostic, UI shows actionable message).
- `app/desktop/src-tauri/src/lib.rs` — same path for the `cargo tauri dev` entry.

**Validators:**
- macOS: `security add-generic-password -s claude-deck -a anthropic_api_key -w sk-ant-…` → relaunch → chat works offline-after-key.
- `keyring` absent → `/api/v1/ai/chat` returns 503 with diagnostic (no panic, no env-var leak path).

---

### C3 — Chat panel + first migration (`isaiah` + `david`)

**Targets:**
- `app/server/core/migrations/0001_chat_conversations.sql` — `CREATE TABLE chat_conversations (id TEXT PRIMARY KEY, title TEXT NOT NULL, messages TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)`.
- `app/server/core/src/lib.rs` — `sqlx::migrate!("./migrations").run(&pool).await?` immediately after pool creation.
- `app/server/core/src/api/v1/chat.rs` — CRUD: `GET /chat` (list), `POST /chat` (create), `GET /chat/{id}`, `PUT /chat/{id}` (append messages), `DELETE /chat/{id}`.
- `app/web/src/features/chat/{ChatPage,ChatList,ChatTranscript,api,types,route,registry-entry}.tsx` — full chat panel using `useChat({ api: '/api/v1/ai/chat', initialMessages: ... })`. Persists transcript via the CRUD routes above.
- Sidebar entry: "Chat" with `MessageSquare` icon, registered through the glob registry.

**Validators:**
- Fresh DB (`rm claude_registry.db && cargo run -p server-bin`) → table created, app starts, chat works.
- Existing DB (Phase-B DB) → migration applies, all Phase-B pages still work, no data loss.
- Playwright nav-smoke for `/chat` route.

---

### C4 — cc-bridge AI augmentation (`isaiah` + `john`)

**Targets:**
- `app/web/src/features/cc-bridge/CCBridgePage.tsx` — split layout: terminal on left, AI panel on right (collapsible; CSS-only on iPhone-375 to keep mobile parity).
- `app/web/src/features/cc-bridge/AISuggestPanel.tsx` — new. Calls `/api/v1/ai/suggest` with terminal context (last N lines from xterm buffer). Renders the suggestion with [Send] / [Edit] / [Discard].
- `useTerminal.ts` — expose `writeStdin(bytes)` so the gate calls into the existing WS stdin path. **No** new bypass path.
- Comment block at the top of `AISuggestPanel.tsx` referencing the INBOX cc-bridge-hardening entry: "the confirm gate is a UX boundary, not auth."

**Validators:**
- Playwright: with a stub server returning `<execute>echo HELLO</execute>`, the preview must render and zero PTY bytes must hit the WS until [Send] is clicked.
- Manual: real Claude session, ask "what command shows disk usage?" → suggestion previews → click Send → `df -h` lands in PTY.
- Mobile (iPhone-375): AI panel collapses to a sheet, terminal remains usable.

---

### C5 — Agents AI-assist (Phase-C template) (`david` + `isaiah`)

**Targets:**
- `app/web/src/features/agents/AgentEditDialog.tsx` (or page equivalent) — add an "✨ AI suggest" affordance.
- `app/web/src/features/agents/AISuggestBlock.tsx` — new. POSTs to `/api/v1/ai/suggest` with `{ messages: [{ role: 'user', content: <user prompt> + <current frontmatter> + <current body> }] }`. Streams into the CM6 buffer; user can keep editing.
- Save path: existing `PUT /api/v1/agents/...` — unchanged.
- Document the template in `app/web/src/features/agents/AI_SUGGEST_PATTERN.md` (short) so C5 is reusable for Commands/Skills/etc. in a future cycle.

**Validators:**
- "Suggest a research assistant agent" → CM6 buffer populated with valid frontmatter + body → Save → file written → reload shows persisted content.
- The `AI_SUGGEST_PATTERN.md` covers: where to mount the affordance, prompt template shape, CM6 onChange contract, save round-trip.

---

### C6 — CM6 for JSON surfaces (`david`) — **REDIRECTED 2026-05-23**

**Original premise was wrong.** MCP and Permissions use structured field editors (Input + Select + Switch), not raw JSON textareas — verified by audit on 2026-05-23. The B4 implementation chose structured forms, a better UX, signed off at Phase-B gate. There is nothing editable to swap in those two features.

**Redirected scope** (PROMPTER decision 2026-05-23): repurpose the slot to the open INBOX item "Extend CodeMirror 6 to read-only config JSON renders".

**Targets:**
- `app/web/src/features/config/ConfigFileViewer.tsx` — swap the read-only JSON render (`JsonViewer` / `<pre>`) for `<CodeEditor language="json" />` in read-only mode. May require extending `CodeEditor` with a `readOnly` prop (one CM6 extension: `EditorState.readOnly.of(true)` + `EditorView.editable.of(false)`).
- Close the corresponding INBOX entry in the same commit.

**Out of scope (unchanged):**
- Adding a "raw JSON mode" toggle to MCP/Permissions edit forms (the option-C path PROMPTER did not pick).

**Validators:**
- `/config` page: JSON config files render with CM6 syntax highlighting, bracket matching, line numbers.
- No editing possible (readonly enforced).
- Playwright: nav smoke green for `/config`.
- INBOX entry "Extend CodeMirror 6 to read-only config JSON renders" marked **closed** (status: done, with commit hash) — not deleted.

---

## Phase C gate (must pass to advance to Phase D)

1. **AI streaming end-to-end.** `useChat` against `/api/v1/ai/chat` streams from Anthropic. DevTools Network on the browser shows **no** `x-api-key` outbound.
2. **Tauri key resolution.** `cargo tauri dev` (or built `.app`) reads key from OS keychain; with the key in keychain and **no** `ANTHROPIC_API_KEY` env var, chat works.
3. **Confirm-before-exec.** With a stub model emitting `<execute>...</execute>`, **zero** PTY bytes until the human clicks Send. (Playwright assertion.)
4. **Agents AI-suggest.** Suggestion lands in CM6 buffer; Save persists via existing PUT.
5. **Migration safety.** `sqlx::migrate!()` applies cleanly to a fresh DB. Data loss on existing DBs is explicitly permitted (PROMPTER policy, 2026-05-22): "quick and destructive upgrade preferred over careful one — not a lot of content on these databases". Future migrations may be destructive (DROP/recreate over careful ALTER).
6. **No regressions.** Playwright sweep that was 61/61 green at Phase-B gate must remain 61/61 + new tests (≥ 65/65 total).
7. **Legacy untouched.** `git diff main -- backend/ frontend/` empty.

---

## Risks

| Risk | Mitigation |
|---|---|
| Vercel AI SDK v6 stream-protocol drift mid-cycle (v6 is recent) | Pin `ai` + `@ai-sdk/react` exact versions in `package.json`. If a stream-frame breaks, downgrade pin before debugging. |
| Anthropic API rate-limit / 429 during dev | Server proxy returns 429 verbatim; client surfaces a toast. No retry-loop in proxy. |
| `sqlx::migrate!` aborting on a DB already touched by Phase B | First migration is purely additive (`CREATE TABLE chat_conversations`). Pre-flight: `cargo run -p server-bin` against a copy of the Phase-B DB before merging C3. |
| C4 split-pane regressing mobile parity | iPhone-375 Playwright spec runs on each C4 commit. AI panel hidden on `<sm` breakpoint, surfaced as a sheet. |
| Streaming responses bloating cc-bridge `useTerminal` re-renders | AI panel state stays in `AISuggestPanel`, not lifted into `useTerminal`. |
| `tauri-plugin-keyring` vs raw `keyring` crate confusion | Use raw crate; never depend on a Tauri plugin for this. Documented in C2. |
| Key-source ambiguity in error UI | 503 body schema `{ status, detail, anthropic_key_configured, key_source: "keyring" | "env" | null }` — UI reads `key_source` to render an actionable hint. |

---

## Build/test commands (Phase C)

```bash
# C1
cd app/server && cargo test -p server-core --test ai_proxy

# C2 (manual)
security add-generic-password -s claude-deck -a anthropic_api_key -w 'sk-ant-…'
cd app/desktop && cargo tauri dev

# C3 (migration safety)
cp app/server/claude_registry.db /tmp/preC3.db
cargo run -p server-bin    # should apply 0001 and start clean
sqlite3 /tmp/preC3.db '.schema chat_conversations'   # confirm table exists post-run

# C4 / C5 / C6
cd app/web && npx playwright test
```

---

## Closure

On gate pass:
- WORKPLAN.md C1–C6 marked ✅ with commit hashes.
- Phase C gate line appended.
- Delete `app/web/src/features/dev-ai/` (A7 leftover repurposed for C1 smoke).
- INBOX entries (cc-bridge auth, design pass, hooks-badge, CM6 for read-only configs) remain **open** — none of them are C-scope.
- ARCHITECT proposes Phase D transition.
