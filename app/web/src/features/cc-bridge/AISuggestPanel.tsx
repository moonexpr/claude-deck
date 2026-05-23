// The Send confirmation is a UX safety boundary, NOT an auth boundary.
// cc-bridge auth hardening (token endpoint, port-exempt origin, etc.)
// is tracked in docs/requests/INBOX.md — "Harden cc-bridge auth surface".

import { useState, useCallback } from 'react'
import { AlertTriangle, Bot, ChevronRight, Send, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { MarkdownRenderer } from '@/components/shared/MarkdownRenderer'
import { cn } from '@/lib/utils'
import { parseExecuteTags } from './parseExecuteTags'
import type { ParsedBlock } from './parseExecuteTags'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface AISuggestPanelProps {
  /** Called with exactly `cmd + "\n"` when the user clicks Send. */
  writeStdin: (s: string) => void
  /** Returns the last ~40 lines of the active xterm buffer as a string. */
  getTerminalContext: () => string
}

interface SuggestResponse {
  text: string
  usage?: { input_tokens: number; output_tokens: number }
}

// ---------------------------------------------------------------------------
// Command preview card
// ---------------------------------------------------------------------------

interface CommandCardProps {
  cmd: string
  index: number
  onSend: (cmd: string) => void
  onEdit: (cmd: string) => void
  onDiscard: (index: number) => void
}

function CommandCard({ cmd, index, onSend, onEdit, onDiscard }: CommandCardProps) {
  return (
    <div
      data-testid="execute-card"
      className="rounded-md border bg-muted/40 px-3 py-2 space-y-2"
    >
      <pre className="text-xs font-mono whitespace-pre-wrap break-all text-foreground leading-5">
        {cmd}
      </pre>
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          className="h-6 text-xs px-2 gap-1"
          onClick={() => onSend(cmd)}
          data-testid="send-button"
        >
          <Send className="h-3 w-3" />
          Send
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="h-6 text-xs px-2"
          onClick={() => onEdit(cmd)}
          data-testid="edit-button"
        >
          Edit
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 text-xs px-2 text-destructive hover:text-destructive"
          onClick={() => onDiscard(index)}
          data-testid="discard-button"
        >
          <Trash2 className="h-3 w-3" />
          Discard
        </Button>
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Main panel
// ---------------------------------------------------------------------------

export function AISuggestPanel({ writeStdin, getTerminalContext }: AISuggestPanelProps) {
  const [prompt, setPrompt] = useState('')
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [unavailable, setUnavailable] = useState(false)
  const [parsedBlocks, setParsedBlocks] = useState<ParsedBlock[]>([])
  /** For the Edit path — pre-fill the prompt textarea. */
  const [editingCmd, setEditingCmd] = useState<string | null>(null)

  const handleSubmit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault()
      const text = prompt.trim()
      if (!text || loading) return

      setLoading(true)
      setErr(null)
      setUnavailable(false)
      setParsedBlocks([])
      setEditingCmd(null)

      const context = getTerminalContext()
      const body = context
        ? `[Terminal context (last 40 lines):]\n${context}\n[User request:]\n${text}`
        : text

      try {
        const response = await fetch('/api/v1/ai/suggest', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            system:
              'You are an assistant attached to a terminal running Claude Code or a shell. The user will describe what they want; you respond with a short explanation followed by shell command(s). Wrap each actionable shell command in <execute></execute> tags exactly. Do not put pipelines, comments, or prose inside the tags — only the literal command to run. Multiple commands → multiple tag pairs.',
            prompt: body,
          }),
        })

        if (response.status === 503) {
          setUnavailable(true)
          return
        }

        if (!response.ok) {
          const data = await response.json().catch(() => ({}))
          throw new Error((data as { message?: string }).message ?? `HTTP ${response.status}`)
        }

        const data = (await response.json()) as SuggestResponse
        setParsedBlocks(parseExecuteTags(data.text))
        setPrompt('')
      } catch (e: unknown) {
        if ((e as { message?: string }).message?.includes('503')) {
          setUnavailable(true)
        } else {
          setErr((e as { message?: string }).message ?? 'Unknown error')
        }
      } finally {
        setLoading(false)
      }
    },
    [prompt, loading, getTerminalContext],
  )

  const handleSend = useCallback(
    (cmd: string) => {
      writeStdin(cmd + '\n')
    },
    [writeStdin],
  )

  const handleEdit = useCallback((cmd: string) => {
    setEditingCmd(cmd)
    setPrompt(cmd)
  }, [])

  const handleDiscard = useCallback((index: number) => {
    setParsedBlocks((prev) => {
      // Remove the execute block at `index` among the execute blocks.
      // We track a running execute counter to find the right element.
      let execCount = -1
      return prev.filter((block) => {
        if (block.kind === 'execute') {
          execCount++
          return execCount !== index
        }
        return true
      })
    })
  }, [])

  // Collect execute-type blocks with stable indices
  const executeBlocks = parsedBlocks.filter((b) => b.kind === 'execute') as Array<{
    kind: 'execute'
    cmd: string
  }>

  return (
    <div className="flex flex-col h-full bg-background border-l">
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2 border-b bg-muted/30 shrink-0">
        <Bot className="h-4 w-4 shrink-0 text-primary" />
        <span className="text-sm font-medium">AI Suggest</span>
        <span className="text-[10px] text-muted-foreground ml-1 hidden sm:inline">
          preview before send
        </span>
      </div>

      {/* Suggestion output */}
      <div className="flex-1 overflow-y-auto min-h-0 px-3 py-2 space-y-2">
        {unavailable && (
          <div className="flex gap-2 items-start rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950/20 dark:text-amber-300">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>
              AI unavailable —{' '}
              <code className="font-mono text-xs">anthropic_api_key</code> not configured. Set it
              in the OS keychain (Tauri) or{' '}
              <code className="font-mono text-xs">ANTHROPIC_API_KEY</code> env (server-bin).
            </span>
          </div>
        )}

        {err && (
          <div className="flex gap-2 items-start rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{err}</span>
          </div>
        )}

        {editingCmd !== null && (
          <div className="text-xs text-muted-foreground px-1">
            Editing command — modify the prompt below and re-send, or clear to cancel.
          </div>
        )}

        {/* Render parsed blocks in order */}
        {parsedBlocks.length > 0 && (
          <div className="space-y-2" data-testid="suggestion-blocks">
            {(() => {
              let execIdx = -1
              return parsedBlocks.map((block, i) => {
                if (block.kind === 'prose') {
                  return block.text.trim() ? (
                    <div key={i} className="text-sm text-foreground/90 leading-relaxed">
                      <MarkdownRenderer content={block.text} />
                    </div>
                  ) : null
                }
                execIdx++
                const capturedIdx = execIdx
                return (
                  <CommandCard
                    key={i}
                    cmd={block.cmd}
                    index={capturedIdx}
                    onSend={handleSend}
                    onEdit={handleEdit}
                    onDiscard={handleDiscard}
                  />
                )
              })
            })()}
          </div>
        )}

        {loading && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground py-2">
            <div className="h-3 w-3 rounded-full border-2 border-primary border-t-transparent animate-spin" />
            Thinking…
          </div>
        )}

        {/* Empty state */}
        {!loading && !err && !unavailable && parsedBlocks.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground text-center py-8">
            <Bot className="h-8 w-8 mb-2 opacity-30" />
            <p className="text-xs">Describe what you want to do.</p>
            <p className="text-xs">Commands preview here before reaching the terminal.</p>
          </div>
        )}
      </div>

      {/* Prompt input */}
      <div className="shrink-0 border-t px-3 py-2">
        {executeBlocks.length > 0 && (
          <p className="text-[10px] text-muted-foreground mb-1">
            {executeBlocks.length} command{executeBlocks.length !== 1 ? 's' : ''} pending — click
            Send to run each one.
          </p>
        )}
        <form onSubmit={handleSubmit} className="flex gap-2 items-end">
          <textarea
            data-testid="ai-prompt-input"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              // Ctrl+Enter or Cmd+Enter submits
              if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
                void handleSubmit(e as unknown as React.FormEvent)
              }
            }}
            placeholder="Ask about the terminal… (Ctrl+Enter to send)"
            rows={2}
            disabled={loading}
            className={cn(
              'flex-1 resize-none rounded-md border bg-background px-3 py-2 text-sm',
              'placeholder:text-muted-foreground focus-visible:outline-none',
              'focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50',
            )}
          />
          <Button
            type="submit"
            size="sm"
            disabled={loading || !prompt.trim()}
            className="h-8 w-8 p-0 shrink-0"
            title="Ask AI"
            data-testid="ai-submit-button"
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </form>
      </div>
    </div>
  )
}
