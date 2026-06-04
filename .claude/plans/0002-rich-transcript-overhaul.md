# Plan 0002 — Rich Transcript Overhaul (+ command-center feature evaluation)

**Session goal:** Investigate command-center for features worth integrating into claude-deck; specifically incorporate rich transcript reporting (showing thinking).

**Status:** Spec → Design. Awaiting approval to enter Dev phase.

---

## 0. Premise correction (investigation result)

- **command-center is not a JSONL transcript viewer.** It is a Supabase-backed chat/ops app; "conversations" are relational DB rows fed by a tmux-router. Its thinking display is inline dimmed `💭` text (no markdown); tool calls/results sit behind a "Show details" disclosure. Architecturally incompatible with, and *simpler than*, claude-deck's existing viewer.
- **claude-deck already parses JSONL and renders thinking** (`session_service.rs` → `features/sessions/`). It also has a **Context tab** and a full **Usage** feature that command-center has no equivalent of.
- **Therefore:** the value is *polishing claude-deck's own transcript view*, borrowing a few narrow patterns from command-center (markdown-in-thinking, lazy highlight, virtualization, tool-trace summary line). This plan does that.

## 1. Grounding facts (verified)

- **Backend passes full content through.** `parse_jsonl_file()` returns raw entries; thinking text, complete tool `input` (incl. Edit `old_string`/`new_string`, `file_path`), tool_result `content`+`is_error`, per-message `usage`, `model`, `timestamp` all reach the client. **All current truncation is display-side** → the core overhaul is **frontend-only**.
  - No thinking `signature` field is surfaced (not needed for rendering).
  - ⚠ Open verification: Rust route `GET /{project}/{session}` appears to return raw `Vec<Value>`, but frontend `getSessionDetail` expects `SessionDetailResponse{session, current_page, total_pages}`. A grouping/pagination layer exists (Rust or `backend_python/`). **Confirm which backend is live in `dev.sh`/`main.rs` before any backend touch.** Does not block frontend work.
- **Usage/cost is already built** (`usage.rs`: tiered price table, daily/monthly/session/blocks, recharts, CSV). Do **not** duplicate; reuse its pricing.
- **CC Bridge streams raw PTY bytes only** — no structured live JSONL stream exists.
- **Deps present:** `react-markdown@10.1.0`, `remark-gfm@4.0.1` (installed, **not wired** into MarkdownRenderer), `react-syntax-highlighter@16.1.1`, `recharts@3.8.1`. **Absent:** `@tanstack/react-virtual`, any diff library.

## 2. Conventions (claude-deck)

- ESLint + TS strict (`noUnusedLocals`/`noUnusedParameters`). Path alias `@/*`.
- Clickable cards → `CLICKABLE_CARD`; modal sizes → `MODAL_SIZES`; markdown → `MarkdownRenderer` / `MarkdownPreviewToggle`.
- No frontend test harness yet → validators are **type-check + lint + manual run** (LDV #1 = execution). Acceptance via `npm run build` (tsc) + `npm run lint` clean + visual verification against a real session.
- Commit signed (`git -c gpg.program=gpg-loopback commit`). No remote push until Build phase.

---

## 3. PHASE A — Full transcript overhaul (frontend-only, implement now)

LDV tasks. Each: localize → design validator → validate. Type-check + lint gate every task; visual check at end of group.

### A1 — Markdown infra hardening
- **Files:** `components/shared/MarkdownRenderer.tsx`.
- Wire `remark-gfm` (tables/strikethrough/task-lists) into `ReactMarkdown` `remarkPlugins`.
- Add a `compact?: boolean` / `variant` prop so thinking can render with tighter prose (smaller margins) without a second renderer.
- **Validator:** renders a GFM table + task list correctly; no eslint unused; build passes.

### A2 — ThinkingBlock: markdown + visibility model
- **Files:** `features/sessions/blocks/ThinkingBlock.tsx`, `ContentBlockRenderer.tsx`.
- Render `thinking` via `MarkdownRenderer` (compact variant) instead of plain `whitespace-pre-wrap`.
- Drop per-block local `collapsed` default; visibility now driven by a **global "Show thinking" toggle** (A3). Keep a per-block collapse affordance but its default state follows the global setting.
- **Validator:** thinking with headers/lists/code renders as markdown; respects global toggle.

### A3 — Global "Show thinking" toggle (default OFF)
- **Files:** `features/sessions/SessionViewPage.tsx` (+ small context or prop thread; prefer a lightweight `SessionViewContext` or `useState` lifted to the page and passed to `ConversationList`→`Conversation`→`Message`→`ContentBlockRenderer`).
- A `Switch` in the session view header: "Show thinking", default off. When off, thinking blocks render collapsed (or hidden behind a slim `💭 thinking` chip); when on, expanded.
- Persist preference to `localStorage` so it survives navigation.
- **Validator:** toggle flips all thinking blocks on the page; persists across reload; default off on first visit.

### A4 — Edit/MultiEdit diff rendering
- **Files:** `features/sessions/blocks/ToolUseBlock.tsx` (+ new `blocks/DiffView.tsx`).
- Add a diff lib (`diff` — lightweight, MIT) → `npm i diff`.
- For `Edit`: render `old_string`→`new_string` as a unified/inline diff with add/remove line coloring; show `file_path`. For `MultiEdit`: list each edit's diff.
- Replace hardcoded `language="typescript"` with **extension-derived language** from `file_path` (map `.ts/.tsx/.py/.rs/.sh/.json/...`).
- For `Write`: keep content preview but with ext-derived language; raise/soften the 500-char cap behind expand.
- **Validator:** an Edit entry shows a colored diff; `.py` file highlights as python, `.rs` as rust.

### A5 — Broaden tool renderers
- **Files:** `ToolUseBlock.tsx` (refactor to a registry/dispatch by tool name + icon map).
- Add compact specialized displays + icons (lucide) for: `Read`, `Grep`, `Glob`, `LS`, `TodoWrite` (render the todo list with status checkboxes), `Task`/`Agent` (subagent prompt + type), `WebFetch`/`WebSearch`, `NotebookEdit`. Unknown tools → existing generic JSON `<pre>`.
- **Validator:** TodoWrite renders a checklist; Grep shows pattern+path; unknown tool still falls back cleanly.

### A6 — ToolResultBlock enrichment
- **Files:** `blocks/ToolResultBlock.tsx`.
- Structured handling: if result is TodoWrite ack → suppress/condense; if it looks like code/command output → monospace with optional syntax highlight; very large → expandable (raise cap, no silent hard cut — show "N more lines").
- Distinguish error styling (already present) and keep it.
- **Validator:** large result expands fully; no information silently lost (no bare 500-char chop).

### A7 — Transcript ergonomics
- **Files:** `SessionViewPage.tsx`, `Conversation.tsx`, `Message.tsx`.
- Add: **Expand-all / Collapse-all** for thinking+tool blocks; per-block/per-message **copy** button (`e.stopPropagation()` per UI conventions); a per-message **tool-trace summary line** ("N tools · M tokens") borrowed from command-center.
- **Validator:** expand-all reveals every collapsible; copy copies the right block; keyboard a11y (Enter/Space) on interactive controls.

### A8 — Performance: virtualization + lazy highlight
- **Files:** `ConversationList.tsx` / `Message` list; `MarkdownRenderer` / tool blocks.
- Add `@tanstack/react-virtual` → `npm i @tanstack/react-virtual`. Virtualize the message/conversation list. **Decision (default):** keep server pagination as-is and virtualize within a page (lower risk); note option to switch to full-session fetch + virtualization later.
- Lazy-load the Prism highlighter (dynamic import; only when a code block / highlightable tool is present), mirroring command-center's `useLazyHighlight`.
- **Validator:** a 500+ message session scrolls without jank; highlighter chunk not loaded on a text-only session (verify via network/devtools or bundle-split).

**Phase A acceptance:** `npm run build` (tsc strict) clean; `npm run lint` clean; manual run (`./scripts/dev.sh`) showing: markdown thinking, working global toggle (default off, persisted), Edit diffs, broadened tools incl. TodoWrite checklist, enriched results, expand/collapse-all + copy, smooth scroll on a large session.

---

## 4. PHASE B — command-center non-transcript features (evaluation + decisions)

User asked to evaluate **live streaming**, **markdown reports**, **usage/cost**. Recommendations below; each gated on a separate decision before any implementation.

### B1 — Usage / cost reporting → **REUSE, don't rebuild**
- Already fully implemented (`usage.rs` + `features/usage/`). Recommendation: **one small enhancement** — surface a **per-session (and optionally per-conversation) cost/token badge inside the transcript view**, computed by reusing the existing pricing table (call `/usage/sessions` or a thin new derive). Low effort, no duplication. Folds naturally after A7.
- **Verdict:** small additive task, recommend YES. Anything larger = duplication, recommend NO.

### B2 — Live session streaming → **feasible but largest lift; propose as future phase**
- CC Bridge gives raw PTY bytes, not structured transcript. A live *transcript* view needs new infra: **tail the session JSONL as Claude writes it** (file watch / incremental read on the active session file) + push deltas over **SSE or WebSocket**, reusing the Phase-A renderers for display.
- **Verdict:** real value (watch a running agent's thinking/tools live) but non-trivial (new backend streaming endpoint + frontend live store). Recommend **deferring to its own plan** after Phase A lands; do not bundle.

### B3 — Markdown reports browser → **needs a source-of-truth decision**
- command-center scans its own `content/reports/*.md`. claude-deck has no analog corpus. Renderer deps are ready (`react-markdown`+`remark-gfm`). Open question: *what would it browse?* (a configurable reports dir? session-associated artifacts? `~/.claude` docs?).
- **Verdict:** low technical risk, but **blocked on product decision** about the source. Recommend a short follow-up to define the corpus, then a small feature. Defer until B-source decided.

---

## 5. Sequencing & gates

1. **Phase A** (A1→A8) — the core ask, frontend-only, implement now under Work↔Eval.
2. **B1** (per-session cost badge) — small, recommend folding in right after A7/A8.
3. **B2 / B3** — separate plans, gated on PROMPTER decision; not in this cycle unless requested.

## 6. Risks / judgment calls

- **Backend identity ambiguity** (Rust vs `backend_python/`): verify the live backend before any backend touch. Phase A avoids it entirely (frontend-only).
- **Virtualization vs pagination**: defaulting to "virtualize within existing pagination" to minimize risk; full-session load is a later option.
- **New deps**: `diff` and `@tanstack/react-virtual` — both small, well-maintained, MIT. Flag for approval.
- **No frontend tests**: validation is tsc+lint+manual. Acceptable per repo state; visual verification is mandatory in Eval.
