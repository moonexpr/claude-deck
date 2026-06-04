import { useLayoutEffect, useRef, useState } from 'react'
import { useWindowVirtualizer } from '@tanstack/react-virtual'
import { Conversation } from './Conversation'
import type { SessionConversation } from '@/types/sessions'

interface Props {
  conversations: SessionConversation[]
}

export function ConversationList({ conversations }: Props) {
  const listRef = useRef<HTMLDivElement>(null)

  // `listRef` is null on the first render, and attaching a ref does not trigger
  // a re-render — so reading `offsetTop` inline leaves scrollMargin at 0 until
  // the first scroll event, which renders every row behind the page header.
  // Capture it in a layout effect (pre-paint) and feed it back as state so the
  // virtualizer positions rows correctly from the first paint.
  const [scrollMargin, setScrollMargin] = useState(0)
  useLayoutEffect(() => {
    setScrollMargin(listRef.current?.offsetTop ?? 0)
  }, [])

  const virtualizer = useWindowVirtualizer({
    count: conversations.length,
    estimateSize: () => 600,
    overscan: 3,
    scrollMargin,
    // Key rows by the conversation's timestamp (a stable per-turn identity)
    // rather than the array index, so collapsible blocks don't inherit a
    // sibling's expanded/collapsed state when the page slice changes.
    getItemKey: (index) => conversations[index].timestamp,
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
