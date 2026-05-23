import { Trash2, Plus } from 'lucide-react'
import { CLICKABLE_CARD } from '@/lib/constants'
import type { ConversationSummary } from './types'

interface ChatListProps {
  conversations: ConversationSummary[]
  activeId: number | null
  onSelect: (id: number) => void
  onDelete: (id: number) => void
  onNew: () => void
  isCreating: boolean
}

/** Format an ISO timestamp as a short relative label. */
function relativeTime(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime()
  const secs = Math.floor(ms / 1000)
  if (secs < 60) return 'just now'
  const mins = Math.floor(secs / 60)
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

export function ChatList({
  conversations,
  activeId,
  onSelect,
  onDelete,
  onNew,
  isCreating,
}: ChatListProps) {
  return (
    <div className="flex flex-col h-full">
      {/* New Chat button */}
      <div className="p-3 border-b border-[hsl(var(--border))]">
        <button
          onClick={onNew}
          disabled={isCreating}
          aria-label="New Chat"
          className="w-full flex items-center gap-2 rounded-md bg-[hsl(var(--primary))] px-3 py-2 text-sm font-medium text-[hsl(var(--primary-foreground))] hover:opacity-90 disabled:opacity-50 transition-opacity"
        >
          <Plus className="h-4 w-4 shrink-0" />
          {isCreating ? 'Creating…' : 'New Chat'}
        </button>
      </div>

      {/* Conversation list */}
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {conversations.length === 0 && (
          <p className="px-2 py-4 text-center text-sm text-[hsl(var(--muted-foreground))] italic">
            No conversations yet.
          </p>
        )}
        {conversations.map((conv) => {
          const isActive = conv.id === activeId
          return (
            <div
              key={conv.id}
              role="button"
              tabIndex={0}
              aria-pressed={isActive}
              onClick={() => onSelect(conv.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  onSelect(conv.id)
                }
              }}
              className={[
                CLICKABLE_CARD,
                'group flex items-start gap-2 rounded-md px-3 py-2',
                isActive
                  ? 'border-primary bg-[hsl(var(--accent))]'
                  : 'border-transparent',
              ].join(' ')}
            >
              {/* Title + metadata */}
              <div className="flex-1 min-w-0">
                <p className="truncate text-sm font-medium leading-snug">
                  {conv.title}
                </p>
                <p className="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                  {conv.message_count} {conv.message_count === 1 ? 'message' : 'messages'} ·{' '}
                  {relativeTime(conv.updated_at)}
                </p>
              </div>

              {/* Delete button */}
              <button
                aria-label={`Delete conversation "${conv.title}"`}
                onClick={(e) => {
                  e.stopPropagation()
                  onDelete(conv.id)
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.stopPropagation()
                    e.preventDefault()
                    onDelete(conv.id)
                  }
                }}
                className="shrink-0 rounded p-1 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:bg-[hsl(var(--destructive)/0.15)] hover:text-[hsl(var(--destructive))] transition-opacity focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          )
        })}
      </div>
    </div>
  )
}
