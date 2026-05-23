# Requests Inbox

Deferred / out-of-scope requests logged during goal sessions, so they are not
lost. See `~/.claude/CLAUDE.md` → Scope Discipline for the logging convention.

---

## 2026-05-22 — Redesign Hooks-page event-type badges with Tailwind v4

**Context:** On the ported Hooks page ("Hooks by Event Type" section,
`app/web/src/features/hooks/HooksPage.tsx`), the event-type filter badge row
renders cramped and overlapping — adjacent labels collide and count bubbles
overrun the text (e.g. "Session End" / "User Prompt Submit" / "Permission
Request" run together; "🔒2Permission Request"). Spotted from a screenshot of
the running app during the Phase B review checkpoint.

**Asked during:** Tauri / portable-pty / CM6 / AI-SDK stack-lift goal session,
branch `lift/tauri-portable-pty-cm6-aisdk`.

**Why deferred:** PROMPTER explicitly flagged it out of scope for the current
goal — a future polish task. The redesign should use the new Tailwind v4
design system (`@theme` tokens, shadcn `Badge`/`Tabs` primitives) rather than
the legacy badge layout that was ported as-is.

**Status:** open

---

## 2026-05-22 — Extend CodeMirror 6 to read-only config JSON renders

**Context:** The Config viewer (`app/web/src/features/config/ConfigFileViewer.tsx`)
renders config files such as `.claude.json` as a plain read-only JSON block
(via the shared `JsonViewer`). PROMPTER suggested giving these read-only config
renders the CodeMirror 6 treatment too (syntax highlighting, folding, etc.).

**Asked during:** Tauri / portable-pty / CM6 / AI-SDK stack-lift goal session,
branch `lift/tauri-portable-pty-cm6-aisdk`, Phase B review checkpoint.

**Why deferred:** PROMPTER framed it as a future enhancement ("maybe ... in the
future"). Note the relation to the canonical plan's **C6** — that task already
schedules CM6 for *editable* JSON surfaces (MCP Servers, Permissions); this
request extends CM6 to the *read-only* config-viewer renders, which C6 does not
currently cover. Whoever picks this up should decide whether to fold it into
C6's scope or keep it a separate follow-up.

**Status:** closed — C6-redirect, commit 8e6db4b

---

## 2026-05-22 — Harden cc-bridge auth surface (deferred security pass)

**Context:** A `john` security review of cc-bridge v2
(`app/server/core/src/cc_bridge/`) during the stack lift surfaced structural
auth-surface issues. The lift's scope was to *preserve* the legacy same-origin +
token mechanism, and it did — these are inherited legacy flaws, not lift
regressions, and the deployment mitigates them (Cloudflare Access in front; LAN
over the Tailscale tailnet). One cheap item — rejecting an absent `Origin`
header — was fixed inline during the lift (commit on `lift/...`); the structural
items below are deferred to a dedicated security pass:

- **Unauthenticated token endpoint** — `GET /api/v1/cc-bridge/token`
  (`cc_bridge/mod.rs` ~L553) has no auth of its own; any client reaching the
  server can mint a 30s terminal token. Decide the auth boundary (rely on CF
  Access / tailnet, or add app-level auth) and document it.
- **localhost-port exemption too broad** — `is_same_origin` (`cc_bridge/mod.rs`)
  exempts any `localhost` / `127.0.0.1` / `[::1]` origin regardless of port, so
  any local process serving a page can open a terminal WS. It cannot be naively
  port-matched: the Tauri webview origin is `tauri://localhost` (no port) while
  the embedded server uses an ephemeral port — a port-match would reject Tauri.
  Needs a deliberate fix (e.g. explicit allowlist of the real frontends).
- **API-wide permissive CORS** — `lib.rs` ~L107 sets `allow_origin(Any)` /
  `allow_methods(Any)` / `allow_headers(Any)` for the whole API. Scope it to the
  known frontends (LAN origin + `tauri://localhost`).
- **Unbounded token store** — the `cc_bridge/mod.rs` token map is only swept on
  issuance; add a size cap / background eviction.
- **No concurrent-session cap** — unlimited simultaneous PTY sessions; add a
  per-client / global cap before spawn.
- **`rand_token` weak fallback** — the non-`/dev/urandom` fallback is
  deterministic; make it a hard error instead of a silent downgrade.

**Asked during:** stack-lift goal session, branch
`lift/tauri-portable-pty-cm6-aisdk`, Phase B.
**Why deferred:** PROMPTER's call — out of the lift's "preserve, don't redesign"
scope; the deployment (CF Access + tailnet) is the current auth boundary. Cheap
hardening was done inline; the structural changes are a separate pass.
**Status:** open

---

## 2026-05-22 — Post-lift design pass: UI density + UX flows

**Context:** After the Phase B page ports landed, the PROMPTER's review flagged
a general design direction (not a specific bug): the UI is "too big" overall —
sizing / density wants tightening across the board — and the "UX loops are near
non-existent" — interaction flows and task loops are underdeveloped, mostly
faithful ports of the legacy screens without their own considered flow.
Specific mobile-UX feedback is reserved by the PROMPTER for a later pass.

**Asked during:** Tauri / portable-pty / CM6 / AI-SDK stack-lift goal session,
branch `lift/tauri-portable-pty-cm6-aisdk`, end of Phase B.

**Why deferred:** Phase B's scope is feature *parity* — a faithful port of the
legacy UI onto the new stack. Density and UX-flow redesign is beyond parity; it
belongs to a dedicated design pass after the lift. Worth pairing with the
already-logged Hooks-badge redesign — both are the same "make it ours, not just
ported" workstream.

**Status:** open
