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
