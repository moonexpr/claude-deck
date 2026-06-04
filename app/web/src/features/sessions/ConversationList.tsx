import { useRef } from 'react'
import { useWindowVirtualizer } from '@tanstack/react-virtual'
import { Conversation } from './Conversation'
import type { SessionConversation } from '@/types/sessions'

interface Props {
  conversations: SessionConversation[]
}

export function ConversationList({ conversations }: Props) {
  const listRef = useRef<HTMLDivElement>(null)

  const virtualizer = useWindowVirtualizer({
    count: conversations.length,
    estimateSize: () => 600,
    overscan: 3,
    scrollMargin: listRef.current?.offsetTop ?? 0,
  })

  const items = virtualizer.getVirtualItems()

  return (
    <div ref={listRef} className="relative" style={{ height: virtualizer.getTotalSize() }}>
      {items.map((vi) => (
        <div
          key={vi.key}
          data-index={vi.index}
          ref={virtualizer.measureElement}
          className="absolute left-0 w-full"
          style={{ transform: `translateY(${vi.start - virtualizer.options.scrollMargin}px)` }}
        >
          <div className="pb-8">
            <Conversation conversation={conversations[vi.index]} />
          </div>
        </div>
      ))}
    </div>
  )
}
