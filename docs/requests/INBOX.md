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

---

## 2026-05-23 — Theme inconsistency in mobile chrome (header + footer)

**Context:** Spotted from an iPhone Safari screenshot hitting the dev server at
`http://100.126.111.105:8000/projects` (tailnet path) during the presence demo
setup. The page **body** renders correctly in light mode — white project cards,
black text, `bg-background` honored — but the **header band** (hamburger /
terminal logo / dark-mode toggle row) and the **footer band** (`Claude Deck
v0.0.1 · GitHub`) render as dark gray against the otherwise light page.

The moon-icon state in the header confirms the app is *currently* set to light
mode (the icon offers to flip *to* dark). So this isn't a user-toggled state —
the chrome surfaces are theme-inconsistent with the body.

**Likely causes (whichever picks up should verify first):**

1. **Hardcoded surface tokens.** Header / footer components in
   `app/web/src/components/layout/` may use a literal `bg-card` /
   `bg-zinc-900` / `bg-slate-800` instead of a `@theme`-bound token that flips
   under `class="dark"`.
2. **`prefers-color-scheme` leak past the `class="dark"` strategy.** The iOS
   device is presumably set to system dark mode. Tailwind v4 with the
   `@theme` directive interacts with `@media (prefers-color-scheme: dark)`
   differently than v3 — if the header/footer's tokens resolve through the
   media query while the body resolves through the class, you get exactly this
   split.

**Where to look first:**
- `app/web/src/components/layout/MainLayout.tsx` (or whatever wraps the page)
- `app/web/src/components/layout/PageHeader.tsx`
- Any sibling `AppFooter` / `SiteFooter` component
- `app/web/src/index.css` `@theme` block — confirm the surface tokens
  (`--background`, `--card`, `--popover`, etc.) have both light + dark values
  and that the `.dark` selector flips them rather than a `@media` query

**Minor adjacent fixes (same component sweep):**
- Card action buttons ("Set Active" / "Remove") inside the project cards
  render as ghost-style with no visible background. Either give them a subtle
  background or use shadcn `Button variant="outline"`.
- Path search input lets Safari spell-check the path (red squiggle under "jc").
  Add `autocorrect="off" autocapitalize="off" spellcheck={false}` to path
  inputs.

**Asked during:** stack-lift goal session, branch
`lift/tauri-portable-pty-cm6-aisdk`, during the presence demo prep
(2026-05-23, post-Phase-D).

**Why deferred:** Demo focus — surfaced via screenshot mid-prep, not blocking
the presence demo (the UI is fully usable, just visually inconsistent on the
chrome bands). Naturally belongs to the broader post-lift design pass
workstream above — file together when picking either up.

**Status:** open

---

## 2026-05-23 — Chat list selected-state: `muted-foreground` clashes with `bg-primary`; plus timestamp unit mismatch

**Context:** Spotted from a screenshot of the Chat panel's `ChatList`
(`app/web/src/features/chat/ChatList.tsx`) during the demo. When a conversation
row is the active/selected one, the card flips to a saturated green background
(`bg-primary`) but the subtitle line (`"1 message · 20576d ago"`) keeps
`text-[hsl(var(--muted-foreground))]` — dim gray. Dim gray on saturated green
fails WCAG and reads as illegible mush.

**Root cause — shadcn token pairing.** Each background token has a paired
foreground token:

| background | foreground |
|---|---|
| `bg-background` | `text-foreground` |
| `bg-card` | `text-card-foreground` |
| `bg-muted` | `text-muted-foreground` |
| `bg-primary` | `text-primary-foreground` |
| `bg-accent` | `text-accent-foreground` |

`text-muted-foreground` is "dim gray suitable for secondary text *on the page's
default background*." The CSS variable resolves to a single gray value
globally — it has no notion of which card it's currently on. When the card
flips to `bg-primary`, the subtitle keeps the global muted gray and clashes.
The `@layer utilities { .text-\[hsl\(var\(--muted-foreground\)\)\] { ... } }`
emission is just Tailwind's JIT output for the arbitrary-value class — not
the cause; it's the symptom of using the wrong token in the active state.

**Two fix paths (pick one):**

1. **Wrong design token.** `bg-primary` is CTA-strength (buttons). shadcn's
   convention for *item selection* is `bg-accent` — subtler, and its paired
   `text-accent-foreground` is legible. If the green is meant to mark "this
   conversation is open," switch the selected-state background from `bg-primary`
   → `bg-accent` and both lines become readable without touching the subtitle
   class.
2. **Keep the green, fix the subtitle.** Override the subtitle when active:
   - `text-muted-foreground data-[state=active]:text-primary-foreground/75`, or
   - Use `text-current/70` on the subtitle and let the parent card's
     `text-primary-foreground` (when active) cascade with 70% alpha.

**Bonus bug in the same screenshot:** `"1 message · 20576d ago"` — that's
~56 years. The server stores `Utc::now().timestamp()` (seconds since epoch,
i64) in `chat_conversations.{created_at,updated_at}`. The client almost
certainly feeds that number directly to `new Date(n)` which expects
**milliseconds**. Fix is one of: (a) `new Date(n * 1000)` on the client; or
(b) change the server inserts/updates to `.timestamp_millis()` (and update the
column comments + any other consumers). Touching the server side also
requires migrating existing rows or accepting that ChatList relative-time
display will look weird for pre-existing rows until a fresh DB.

**Where to look:**
- Selected state: `app/web/src/features/chat/ChatList.tsx` (the active-row
  className)
- Subtitle class: same file
- Timestamp render: same file, the `relativeTime(updated_at)` (or similar)
  call site
- Server timestamps: `app/server/core/src/api/v1/chat.rs` —
  `Utc::now().timestamp()` calls in `create_conversation` and
  `update_conversation`

**Asked during:** stack-lift goal session, branch
`lift/tauri-portable-pty-cm6-aisdk`, during the presence demo prep
(2026-05-23, post-Phase-D).

**Why deferred:** Same reason as the mobile-chrome theme entry above — demo
focus, not blocking; bundles cleanly with the post-lift design pass.

**Status:** open
