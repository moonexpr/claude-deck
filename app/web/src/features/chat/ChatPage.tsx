import { useState, useEffect, useCallback, useRef } from 'react'
import { useChat } from '@ai-sdk/react'
import { DefaultChatTransport } from 'ai'
import type { UIMessage } from 'ai'
import { ArrowLeft, AlertTriangle } from 'lucide-react'
import { ChatList } from './ChatList'
import { ChatTranscript } from './ChatTranscript'
import {
  listConversations,
  getConversation,
  createConversation,
  updateConversation,
  deleteConversation,
} from './api'
import type { ConversationSummary, Message } from './types'

/** Shape of the 503 `ErrorBody` from `/api/v1/ai/chat` we care about. */
type UnavailableInfo = {
  keySource: 'oauth' | 'api_key' | null
  detail: string
}

/** Pull the JSON `ErrorBody` out of a useChat error.message string. */
function parseUnavailableInfo(rawMessage: string): UnavailableInfo | null {
  const start = rawMessage.indexOf('{')
  const end = rawMessage.lastIndexOf('}')
  if (start < 0 || end <= start) return null
  try {
    const body = JSON.parse(rawMessage.slice(start, end + 1)) as {
      status?: string
      detail?: string
      key_source?: 'oauth' | 'api_key' | null
    }
    if (body.status !== 'unavailable') return null
    return {
      keySource: body.key_source ?? null,
      detail: body.detail ?? '',
    }
  } catch {
    return null
  }
}

/** Strip useChat-local fields (id, createdAt) before persisting to server. */
function toServerMessages(uiMessages: UIMessage[]): Message[] {
  return uiMessages
    .filter((m) => m.role === 'user' || m.role === 'assistant')
    .map((m) => ({
      role: m.role as 'user' | 'assistant',
      content: m.parts
        .filter((p): p is Extract<typeof p, { type: 'text' }> => p.type === 'text')
        .map((p) => p.text)
        .join(''),
    }))
}

/** Derive a conversation title from the first user message (max 40 chars). */
function deriveTitle(uiMessages: UIMessage[]): string {
  const first = uiMessages.find((m) => m.role === 'user')
  if (!first) return 'New Chat'
  const text = first.parts
    .filter((p): p is Extract<typeof p, { type: 'text' }> => p.type === 'text')
    .map((p) => p.text)
    .join('')
    .trim()
  return text.length > 40 ? text.slice(0, 40) + '…' : text || 'New Chat'
}

export function ChatPage() {
  const [conversations, setConversations] = useState<ConversationSummary[]>([])
  const [activeId, setActiveId] = useState<number | null>(null)
  const [isCreating, setIsCreating] = useState(false)
  const [unavailableInfo, setUnavailableInfo] = useState<UnavailableInfo | null>(null)
  /** On mobile: whether the sidebar is showing (vs the transcript). */
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(true)
  /** Tracks whether the auto-title has already fired for the active conversation. */
  const autoTitledRef = useRef<Set<number>>(new Set())

  // --- Persistence: load conversation list on mount ---
  useEffect(() => {
    listConversations()
      .then((res) => setConversations(res.conversations))
      .catch(() => {
        // Non-fatal — user can still create new conversations.
      })
  }, [])

  // --- useChat: streaming ---
  const { messages, setMessages, sendMessage, status, error } = useChat({
    transport: new DefaultChatTransport({ api: '/api/v1/ai/chat' }),
    onFinish: useCallback(
      ({ messages: finishedMessages }: { messages: UIMessage[] }) => {
        if (activeId === null) return

        const serverMsgs = toServerMessages(finishedMessages)
        const patch: { messages: Message[]; title?: string } = {
          messages: serverMsgs,
        }

        // Auto-title: derive once from the first user message.
        if (!autoTitledRef.current.has(activeId)) {
          const firstUser = finishedMessages.find((m) => m.role === 'user')
          if (firstUser) {
            const title = deriveTitle(finishedMessages)
            patch.title = title
            autoTitledRef.current.add(activeId)
          }
        }

        updateConversation(activeId, patch)
          .then((updated) => {
            setConversations((prev) =>
              prev.map((c) =>
                c.id === updated.id
                  ? {
                      id: updated.id,
                      title: updated.title,
                      message_count: updated.messages.length,
                      created_at: updated.created_at,
                      updated_at: updated.updated_at,
                    }
                  : c,
              ),
            )
          })
          .catch(() => {
            // Persistence failure is non-fatal; messages remain in local state.
          })
      },
      [activeId],
    ),
  })

  // Detect 503 from the streaming endpoint (no usable credential).
  // The server's ErrorBody carries `key_source: "oauth" | "api_key" | null`,
  // so the banner can tell the user *which* path is broken — OAuth (launch
  // `claude_ext` first) vs API key (set ANTHROPIC_API_KEY / keychain) vs
  // nothing configured.
  useEffect(() => {
    if (!error) return
    const msg = error.message ?? ''
    const is503 = msg.includes('503') || msg.toLowerCase().includes('service unavailable')
    const parsed = parseUnavailableInfo(msg)
    if (parsed) {
      setUnavailableInfo(parsed)
    } else if (is503) {
      // Fall back to the unknown-source variant when we can't parse the body.
      setUnavailableInfo({ keySource: null, detail: '' })
    }
  }, [error])

  // --- Actions ---

  const handleSelect = useCallback(
    async (id: number) => {
      if (id === activeId) {
        setMobileSidebarOpen(false)
        return
      }
      try {
        const conv = await getConversation(id)
        // Reconstruct UIMessages from server messages for useChat.
        const uiMessages: UIMessage[] = conv.messages.map((m, i) => ({
          id: `loaded-${id}-${i}`,
          role: m.role,
          parts: [{ type: 'text' as const, text: m.content }],
          metadata: {},
        }))
        setMessages(uiMessages)
        setActiveId(id)
        setMobileSidebarOpen(false)
      } catch {
        // Non-fatal — just don't switch.
      }
    },
    [activeId, setMessages],
  )

  const handleNew = useCallback(async () => {
    setIsCreating(true)
    try {
      const conv = await createConversation('New Chat', [])
      const summary: ConversationSummary = {
        id: conv.id,
        title: conv.title,
        message_count: 0,
        created_at: conv.created_at,
        updated_at: conv.updated_at,
      }
      setConversations((prev) => [summary, ...prev])
      setMessages([])
      setActiveId(conv.id)
      setMobileSidebarOpen(false)
    } catch {
      // Non-fatal.
    } finally {
      setIsCreating(false)
    }
  }, [setMessages])

  const handleDelete = useCallback(
    async (id: number) => {
      try {
        await deleteConversation(id)
        setConversations((prev) => prev.filter((c) => c.id !== id))
        if (id === activeId) {
          setActiveId(null)
          setMessages([])
          setMobileSidebarOpen(true)
        }
      } catch {
        // Non-fatal.
      }
    },
    [activeId, setMessages],
  )

  const [inputValue, setInputValue] = useState('')

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault()
      const text = inputValue.trim()
      if (!text || status === 'streaming' || status === 'submitted') return
      setInputValue('')
      void sendMessage({ text })
    },
    [inputValue, status, sendMessage],
  )

  const isBusy = status === 'streaming' || status === 'submitted'

  // --- Sidebar component (shared between desktop and mobile) ---
  const sidebar = (
    <div className="flex flex-col h-full border-r border-[hsl(var(--border))] bg-[hsl(var(--sidebar,var(--background)))]">
      <ChatList
        conversations={conversations}
        activeId={activeId}
        onSelect={handleSelect}
        onDelete={handleDelete}
        onNew={handleNew}
        isCreating={isCreating}
      />
    </div>
  )

  const transcript = (
    <div className="flex flex-col h-full">
      {/* Mobile back button */}
      <div className="sm:hidden flex items-center gap-2 px-3 py-2 border-b border-[hsl(var(--border))]">
        <button
          onClick={() => setMobileSidebarOpen(true)}
          className="flex items-center gap-1.5 text-sm text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] transition-colors"
          aria-label="Back to conversations"
        >
          <ArrowLeft className="h-4 w-4" />
          Conversations
        </button>
      </div>

      {/* 503 / no-credential banner — copy branches on the server's key_source. */}
      {unavailableInfo && (
        <div className="mx-3 mt-3 flex gap-2 items-start rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950/20 dark:text-amber-300">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          {unavailableInfo.keySource === 'oauth' ? (
            <span>
              AI chat unavailable — no Claude Code OAuth bearer observed yet. Run{' '}
              <code className="font-mono text-xs">claude_ext</code> (or launch Claude Code
              once through it) so <code className="font-mono text-xs">claudecode_ext</code>{' '}
              can capture the bearer.
            </span>
          ) : unavailableInfo.keySource === 'api_key' ? (
            <span>
              AI chat unavailable — <code className="font-mono text-xs">anthropic_api_key</code>{' '}
              not configured. Set it in the OS keychain (Tauri) or{' '}
              <code className="font-mono text-xs">ANTHROPIC_API_KEY</code> env (server-bin).
            </span>
          ) : (
            <span>
              AI chat unavailable — no credential source configured. Either set{' '}
              <code className="font-mono text-xs">ANTHROPIC_API_KEY</code> (server-bin) /
              keychain (Tauri), or enable the OAuth path via{' '}
              <code className="font-mono text-xs">claudecode_ext</code> (set{' '}
              <code className="font-mono text-xs">CLAUDECODE_EXT_KEY_SOURCE=oauth</code>).
            </span>
          )}
        </div>
      )}

      <ChatTranscript
        messages={messages}
        input={inputValue}
        onInputChange={setInputValue}
        onSubmit={handleSubmit}
        isBusy={isBusy}
        conversationId={activeId}
      />
    </div>
  )

  return (
    <div className="flex flex-col h-full">
      {/* Page title row */}
      <div className="px-4 py-3 border-b border-[hsl(var(--border))]">
        <h1 className="text-xl font-semibold">Chat</h1>
      </div>

      {/* Two-pane layout: sidebar + transcript */}
      <div className="flex flex-1 overflow-hidden">
        {/* Desktop: always show both panes */}
        <div className="hidden sm:flex sm:w-64 lg:w-72 flex-col shrink-0 overflow-hidden">
          {sidebar}
        </div>

        {/* Mobile: show sidebar OR transcript, not both */}
        <div className="flex sm:hidden flex-col flex-1 overflow-hidden">
          {mobileSidebarOpen ? sidebar : transcript}
        </div>

        {/* Desktop transcript */}
        <div className="hidden sm:flex flex-1 flex-col overflow-hidden">
          {transcript}
        </div>
      </div>
    </div>
  )
}
