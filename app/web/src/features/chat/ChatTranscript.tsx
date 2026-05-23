import { useRef, useEffect } from 'react'
import type { UIMessage } from 'ai'
import { MarkdownRenderer } from '@/components/shared/MarkdownRenderer'
import { Send } from 'lucide-react'

interface ChatTranscriptProps {
  messages: UIMessage[]
  input: string
  onInputChange: (value: string) => void
  onSubmit: (e: React.FormEvent) => void
  isBusy: boolean
  conversationId: number | null
}

/** Extract plain text from a UIMessage's parts array. */
function messageText(msg: UIMessage): string {
  return msg.parts
    .filter((p): p is Extract<typeof p, { type: 'text' }> => p.type === 'text')
    .map((p) => p.text)
    .join('')
}

export function ChatTranscript({
  messages,
  input,
  onInputChange,
  onSubmit,
  isBusy,
  conversationId,
}: ChatTranscriptProps) {
  const bottomRef = useRef<HTMLDivElement>(null)

  // Auto-scroll to bottom when messages change.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  if (conversationId === null) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-[hsl(var(--muted-foreground))] italic">
        Select a conversation or create a new one.
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full">
      {/* Message list */}
      <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
        {messages.length === 0 && (
          <p className="text-center text-sm text-[hsl(var(--muted-foreground))] italic mt-8">
            No messages yet. Start the conversation below.
          </p>
        )}
        {messages.map((msg) => {
          const text = messageText(msg)
          const isUser = msg.role === 'user'
          return (
            <div
              key={msg.id}
              className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}
            >
              <div
                className={[
                  'max-w-[80%] rounded-lg px-4 py-2.5 text-sm',
                  isUser
                    ? 'bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))]'
                    : 'border border-[hsl(var(--border))] bg-[hsl(var(--card))]',
                ].join(' ')}
              >
                {isUser ? (
                  <p className="whitespace-pre-line">{text}</p>
                ) : (
                  <MarkdownRenderer content={text} />
                )}
              </div>
            </div>
          )
        })}
        {/* Streaming indicator */}
        {isBusy && (
          <div className="flex justify-start">
            <div className="rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--card))] px-4 py-2.5">
              <span className="inline-flex gap-1 items-center text-sm text-[hsl(var(--muted-foreground))]">
                <span className="animate-pulse">●</span>
                <span className="animate-pulse [animation-delay:150ms]">●</span>
                <span className="animate-pulse [animation-delay:300ms]">●</span>
              </span>
            </div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      {/* Input form */}
      <form
        onSubmit={onSubmit}
        className="border-t border-[hsl(var(--border))] p-3 flex gap-2 items-end"
      >
        <textarea
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={(e) => {
            // Submit on Enter (without Shift)
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              onSubmit(e as unknown as React.FormEvent)
            }
          }}
          placeholder="Type a message… (Enter to send, Shift+Enter for newline)"
          rows={1}
          disabled={isBusy}
          className="flex-1 resize-none rounded-md border border-[hsl(var(--border))] bg-[hsl(var(--background))] px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-[hsl(var(--ring))] disabled:opacity-50 max-h-40 overflow-y-auto"
          style={{ fieldSizing: 'content' } as React.CSSProperties}
        />
        <button
          type="submit"
          disabled={isBusy || !input.trim()}
          aria-label="Send message"
          className="shrink-0 rounded-md bg-[hsl(var(--primary))] p-2 text-[hsl(var(--primary-foreground))] hover:opacity-90 disabled:opacity-50 transition-opacity"
        >
          <Send className="h-4 w-4" />
        </button>
      </form>
    </div>
  )
}
