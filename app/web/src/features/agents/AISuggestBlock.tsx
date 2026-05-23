/**
 * AISuggestBlock — AI-assisted content generation for the Agents editor.
 *
 * Phase C, task C5.  This is the canonical template for AI-assisted config
 * editing across Claude Deck.  See AI_SUGGEST_PATTERN.md for the reuse guide.
 *
 * Props
 * -----
 *   currentValue  The current CM6 buffer text (read-only; used to build the prompt).
 *   onAccept      Called with the suggested text when the user clicks "Use this".
 *                 The caller replaces its buffer by passing this value to its
 *                 existing state setter (e.g. setPrompt).  No CM6 internals
 *                 are touched here.
 *
 * UX
 * --
 *   Button "AI suggest" toggles a panel open/closed.
 *   Panel has a textarea (multi-line; no auto-submit on Enter), a Submit button,
 *   a preview area, and a "Use this" + "Discard" button pair.
 *   Submitting replaces the preview; "Use this" calls onAccept and collapses
 *   the panel.
 *   503 (no API key) shows a non-blocking amber banner.
 */

import { useState, useRef } from 'react'
import { Sparkles, AlertTriangle, ChevronDown, ChevronUp } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { apiClient } from '@/lib/api'

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const AGENT_SYSTEM_PROMPT =
  'You are generating or improving a Claude Code agent definition file. ' +
  'The file has YAML frontmatter (`name:`, `description:`, optional `model:`, `tools:`) ' +
  'followed by markdown body content that defines the agent\'s persona and instructions. ' +
  'Output ONLY the file content — no preamble, no markdown fences, no commentary. ' +
  'Preserve existing frontmatter fields if present in the current content.'

// ---------------------------------------------------------------------------
// API call
// ---------------------------------------------------------------------------

interface SuggestResponse {
  text: string
  usage: { input_tokens: number; output_tokens: number }
}

async function callSuggest(
  currentValue: string,
  userRequest: string,
): Promise<SuggestResponse> {
  // The /suggest endpoint accepts { messages: [{role, content}] }.
  // The server's Message type now has a first-class "system" role, so we
  // send the system prompt as a separate system message rather than packing
  // it into the first user turn.
  const userContent = [
    'Current file content:',
    '```',
    currentValue,
    '```',
    '',
    'User request:',
    userRequest,
  ].join('\n')

  return apiClient<SuggestResponse>('ai/suggest', {
    method: 'POST',
    body: JSON.stringify({
      messages: [
        { role: 'system', content: AGENT_SYSTEM_PROMPT },
        { role: 'user', content: userContent },
      ],
    }),
  })
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface AISuggestBlockProps {
  /** The current CM6 buffer value — passed into the prompt as context. */
  currentValue: string
  /**
   * Called with the suggested text when the user clicks "Use this".
   * The caller should pass this directly to its existing state setter.
   */
  onAccept: (newValue: string) => void
}

type PanelState = 'closed' | 'input' | 'preview'

export function AISuggestBlock({ currentValue, onAccept }: AISuggestBlockProps) {
  const [panelState, setPanelState] = useState<PanelState>('closed')
  const [request, setRequest] = useState('')
  const [suggestion, setSuggestion] = useState('')
  const [loading, setLoading] = useState(false)
  const [unavailable, setUnavailable] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Keep textarea focused after submit so keyboard users don't lose context.
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  function handleToggle() {
    if (panelState === 'closed') {
      setPanelState('input')
      setUnavailable(false)
      setError(null)
    } else {
      setPanelState('closed')
    }
  }

  async function handleSubmit() {
    const trimmed = request.trim()
    if (!trimmed || loading) return

    setLoading(true)
    setUnavailable(false)
    setError(null)

    try {
      const resp = await callSuggest(currentValue, trimmed)
      setSuggestion(resp.text)
      setPanelState('preview')
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('503') || msg.toLowerCase().includes('service unavailable')) {
        setUnavailable(true)
      } else {
        setError(msg)
      }
    } finally {
      setLoading(false)
    }
  }

  function handleUse() {
    onAccept(suggestion)
    setPanelState('closed')
    setRequest('')
    setSuggestion('')
  }

  function handleDiscard() {
    setPanelState('input')
    setSuggestion('')
  }

  const isOpen = panelState !== 'closed'

  return (
    <div className="space-y-2">
      {/* Toggle button */}
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={handleToggle}
        className="flex items-center gap-1.5 text-sm"
        aria-expanded={isOpen}
        aria-controls="ai-suggest-panel"
      >
        <Sparkles className="h-3.5 w-3.5" />
        AI suggest
        {isOpen ? (
          <ChevronUp className="h-3.5 w-3.5 ml-1" />
        ) : (
          <ChevronDown className="h-3.5 w-3.5 ml-1" />
        )}
      </Button>

      {/* Panel */}
      {isOpen && (
        <div
          id="ai-suggest-panel"
          className="rounded-md border border-dashed border-primary/40 bg-muted/20 p-3 space-y-3"
          role="region"
          aria-label="AI suggestion panel"
        >
          {/* 503 banner */}
          {unavailable && (
            <div
              role="alert"
              className="flex gap-2 items-start rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950/20 dark:text-amber-300"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                AI suggest unavailable —{' '}
                <code className="font-mono text-xs">anthropic_api_key</code> not
                configured. Set it in the OS keychain (Tauri) or{' '}
                <code className="font-mono text-xs">ANTHROPIC_API_KEY</code> env
                (server-bin). You can still edit and save manually.
              </span>
            </div>
          )}

          {/* General error */}
          {error && (
            <div
              role="alert"
              className="flex gap-2 items-start rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}

          {/* Input area — always visible when panel is open (including after preview) */}
          {panelState === 'input' && (
            <div className="space-y-2">
              <Label htmlFor="ai-suggest-request" className="text-sm font-medium">
                What would you like the AI to do?
              </Label>
              <Textarea
                id="ai-suggest-request"
                ref={textareaRef}
                value={request}
                onChange={(e) => setRequest(e.target.value)}
                placeholder="e.g. Make this a research assistant focused on web search"
                className="min-h-[72px] resize-y text-sm"
                disabled={loading}
              />
              <Button
                type="button"
                size="sm"
                onClick={handleSubmit}
                disabled={!request.trim() || loading}
                className="w-full sm:w-auto"
              >
                {loading ? 'Generating...' : 'Generate suggestion'}
              </Button>
            </div>
          )}

          {/* Preview area */}
          {panelState === 'preview' && (
            <div className="space-y-3">
              <div className="space-y-1">
                <Label className="text-sm font-medium text-muted-foreground">
                  Suggested content (preview):
                </Label>
                <pre className="rounded-md border bg-muted/30 p-3 text-xs leading-relaxed overflow-auto max-h-48 whitespace-pre-wrap break-words font-mono">
                  {suggestion}
                </pre>
              </div>

              <div className="flex gap-2 flex-wrap">
                <Button
                  type="button"
                  size="sm"
                  onClick={handleUse}
                  className="flex-shrink-0"
                >
                  Use this
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleDiscard}
                  className="flex-shrink-0"
                >
                  Discard — try again
                </Button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
