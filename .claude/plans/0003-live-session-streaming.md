# Plan 0003 — Live Session Transcript Streaming (B2 spec)

**Status:** Design/Spec only. No implementation. Successor to Plan 0002 Phase A.
**Depends on:** moonexpr/claude-deck#7 (Rust `get_session_detail` grouping) — see §6.

## 1. Goal

Watch a *running* Claude Code session's transcript live in claude-deck — thinking, tool calls, and results appear as Claude writes them — reusing the Plan-0002 block renderers. command-center has an analog (live interleaved rows from its tmux-router); claude-deck currently streams only **raw PTY bytes** through CC Bridge, not a structured transcript.

## 2. Why this is a separate, larger lift

CC Bridge (`app/server/core/src/cc_bridge/`) relays terminal bytes over a WebSocket (`pty.rs` read loop → mpsc → ws). That gives a *terminal*, not a *transcript*. A live transcript needs a new server path that **tails the session's JSONL file** and pushes structured deltas. New backend endpoint + new frontend live store; no reuse of the PTY byte channel.

## 3. Session-identity resolution (the crux — verified feasible)

From `cc_bridge/mod.rs`:
- `spawned_sessions(): HashMap<String, SpawnMeta>` tracks each session; every spawn records **`cwd`**.
- `SpawnRequest` carries `session_id` + `project_folder`; **resume mode** passes a known `session_id` (`--resume`).
- `resolve_project_directory()` already maps `project_folder` (`-Users-jc-foo`) ↔ filesystem path.

Therefore the live JSONL is locatable:
- **Resume mode:** `session_id` is known → path is `~/.claude/projects/<enc(cwd)>/<session_id>.jsonl` directly.
- **Fresh (plain/worktree):** `session_id` is minted by the `claude` CLI, not known at spawn. Resolve by watching `~/.claude/projects/<enc(cwd)>/` and binding to the **newest `.jsonl` created/modified after spawn time**. Record the bound `session_id` into `SpawnMeta` once observed so reconnects are deterministic.

> Edge: multiple fresh sessions in one cwd racing. Mitigation: bind on first post-spawn file whose first line's `sessionId` isn't already bound; persist the binding.

## 4. Transport: SSE (recommended) over WebSocket

The transcript stream is **server→client only** (no client input on this channel — input goes through the existing PTY ws). Server-Sent Events fit better than WS: simpler, auto-reconnect with `Last-Event-ID`, works through the same socket/proxy. Axum supports SSE natively (`axum::response::sse`).

- **Endpoint:** `GET /api/v1/cc-bridge/sessions/{target}/transcript/stream` (token-vended + same-origin, mirroring the existing terminal token guard).
- **Event:** `data:` = one grouped `SessionMessage` (or conversation delta) in the **same typed-block shape** the renderers consume; `id:` = byte offset or line number for resume.
- **Backfill:** on connect, emit the existing tail of the file (last N messages) as initial events, then stream appends.

## 5. Backend design

- **File watch:** `notify` crate (cross-platform FS events) on the bound `.jsonl`, debounced; fall back to 500ms poll of file length where FS events are unreliable (network mounts).
- **Incremental parse:** track byte offset; on growth, read appended complete lines, parse each JSONL entry, run the **same grouping/reshape used by #7** to produce typed blocks. Factor that reshape into a shared `session_service` function so detail-view and live-stream share one implementation (single source of truth).
- **Lifecycle:** stream ends (SSE close event) when the bound session leaves `spawned_sessions()` (killed) or the file is idle past a TTL.

## 6. Hard dependency on #7

#7 makes `get_session_detail` group raw entries into `SessionConversation`/`SessionMessage`/typed `ContentBlock`s. **B2 must emit that identical shape** so the Plan-0002 renderers work unchanged. Implement #7 first and extract the reshape as a reusable function; B2 calls it per appended entry. Building B2 before #7 would duplicate the reshape and risk drift. → B2 is **blocked-by #7**.

## 7. Frontend design

- **Where:** add a "Live" affordance on the **CC Bridge** page (it already lists running sessions) that opens a transcript pane beside/over the terminal; or a `?live=1` mode on `SessionViewPage`.
- **Live store:** `EventSource` → append messages to a growing array; reuse `ConversationList`/`Message`/`ContentBlockRenderer` verbatim. Wrap in the existing `TranscriptContext` (Show-thinking toggle applies live too).
- **UX:** "● LIVE" pulse indicator; sticky auto-scroll-to-bottom with a "jump to latest" button when the user scrolls up; reconnect via `EventSource` built-in retry + `Last-Event-ID`.
- **Virtualization:** the Plan-0002 `useWindowVirtualizer` already bounds DOM; appending works with dynamic measurement.

## 8. Risks / open questions

- **Fresh-session id binding race** (§3) — needs the persist-on-first-observe mitigation; verify with two concurrent spawns in one cwd.
- **JSONL partial-line writes** — only parse complete newline-terminated lines; hold a partial-line buffer across reads.
- **`notify` reliability on the unix-socket/nginx deployment** — keep the poll fallback.
- **Compaction / file rewrite** — if Claude rewrites the file (summary/compaction), offset tracking must detect truncation (file shorter than offset → re-backfill).
- **Auth surface** — reuse the exact token-vending + same-origin checks already in `cc_bridge`; do not open an unauthenticated stream.

## 9. Effort estimate & phasing

| Step | Effort |
|------|--------|
| #7 reshape extraction (prereq) | M |
| Backend: id-binding + file-tail + incremental parse | M–L |
| Backend: SSE endpoint + token guard + backfill/resume | M |
| Frontend: EventSource store + live pane + indicators | M |
| Hardening: races, truncation, poll fallback | M |

Recommend a dedicated session after #7 lands. Not part of Plan 0002.
