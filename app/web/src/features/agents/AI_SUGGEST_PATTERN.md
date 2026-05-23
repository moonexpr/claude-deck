# AI Suggest Pattern — Phase C Reuse Guide

The `AISuggestBlock` component (first implemented in `features/agents/`) is the
canonical template for adding AI-assisted content generation to any config
editor in Claude Deck. This document captures the design decisions so the
pattern can be transplanted to Commands, Skills, Memory, Output Styles, and
Hooks with minimal thought.

---

## Where to Mount

Place `<AISuggestBlock>` **next to the editor label**, not inside the CM6
editor itself. The pattern used in `AgentEditor.tsx`:

```tsx
<div className="flex items-center justify-between">
  <Label>System Prompt</Label>
  <AISuggestBlock currentValue={value} onAccept={setValue} />
</div>
<MarkdownPreviewToggle value={value} onChange={setValue} ... />
```

The block manages its own open/closed state — it collapses after "Use this" and
does not disturb the surrounding layout when closed.

---

## System-Prompt Shape Per File Kind

The system prompt is sent as a first-class `system`-role message. The server's
`Message` enum supports `system | user | assistant`, matching the Anthropic
messages API. Template:

```
You are generating or improving a Claude Code <FILE_KIND> file.
<One sentence describing the file's expected structure.>
Output ONLY the file content — no preamble, no markdown fences, no commentary.
Preserve existing fields if present in the current content.
```

| Feature | `<FILE_KIND>` phrase | Structure note |
|---------|----------------------|----------------|
| Agents | `agent definition file` | YAML frontmatter + markdown body |
| Commands | `slash command definition file` | YAML frontmatter + markdown body |
| Skills | `skill definition file` | YAML frontmatter + markdown body |
| Memory | `memory file` | Markdown only; no frontmatter |
| Output Styles | `output style definition file` | YAML frontmatter + markdown body |
| Hooks | `hook script` | Shell script or JSON config |

---

## Buffer Write Contract

`onAccept` **replaces** the entire buffer — it is not an append or a merge.

The caller passes its existing state setter directly:

```tsx
<AISuggestBlock currentValue={value} onAccept={setValue} />
```

`setValue` is whatever setter drives the `MarkdownPreviewToggle` (or plain
`<CodeEditor>`). The component never touches CM6 internals; it just calls
`onAccept(newText)` and the existing controlled-value wiring does the rest.

The user can edit freely after accepting — the suggestion is just a starting
point, not a locked value.

---

## Save Round-Trip

`AISuggestBlock` has no knowledge of the save path. The flow is:

1. User clicks "Use this" → `onAccept(suggestion)` → caller state updated.
2. Caller's existing Save button fires → `PUT /api/v1/<feature>/<id>` as usual.

No new server routes. No special save trigger inside `AISuggestBlock`.

---

## 503 Handling

When `POST /api/v1/ai/suggest` returns 503 (no API key configured), render
the amber banner pattern used in `ChatPage.tsx`:

```tsx
<div role="alert" className="... border-amber-300 bg-amber-50 ...">
  <AlertTriangle ... />
  <span>
    AI suggest unavailable — <code>anthropic_api_key</code> not configured.
    Set it in the OS keychain (Tauri) or <code>ANTHROPIC_API_KEY</code> env
    (server-bin). You can still edit and save manually.
  </span>
</div>
```

The banner is non-blocking — the panel stays open, the user can retry or
dismiss it by closing the panel. The editor and Save button remain fully
functional.

---

## Reuse Checklist

Five steps to add `AISuggestBlock` to a new feature:

1. **Copy the component.** Copy `AISuggestBlock.tsx` into the target feature
   directory (e.g. `features/commands/AISuggestBlock.tsx`) or import the
   agents version directly — both work. If you copy, update `AGENT_SYSTEM_PROMPT`
   to the appropriate file-kind wording from the table above.

2. **Identify the value + setter.** Find the state pair that drives the
   primary content editor (`value` / `onChange`) — typically
   `const [content, setContent] = useState('')`.

3. **Mount next to the label.** Wrap the label in a flex row and add
   `<AISuggestBlock currentValue={content} onAccept={setContent} />` to
   the right side.

4. **Verify no new imports are needed.** `AISuggestBlock` only needs
   `@/components/ui/{button,textarea,label}`, `@/lib/api`, and standard
   React hooks — all already present in every feature.

5. **Smoke-test manually.** Open the editor, click "AI suggest", type a
   request, click "Generate suggestion", verify the preview appears, click
   "Use this", verify the buffer updates, save, reload, confirm persistence.
   No AI unit tests are required.
