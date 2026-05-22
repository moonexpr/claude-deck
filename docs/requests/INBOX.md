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

**Status:** open
