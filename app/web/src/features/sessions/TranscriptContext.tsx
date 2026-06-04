import { createContext, useContext } from 'react'

/**
 * Shared display state for a transcript view, consumed by the block
 * renderers without prop-drilling through ConversationList → Conversation
 * → Message → ContentBlockRenderer.
 *
 * - `showThinking` is the global "Show thinking" toggle (default off).
 *   Thinking blocks use it as their default collapsed state.
 * - `expandSignal` is a broadcast for Expand-all / Collapse-all. Blocks
 *   watch `expandSignal.nonce`; when it changes they reset their local
 *   collapsed state to `!expandSignal.expand`.
 */
export interface ExpandSignal {
  expand: boolean
  nonce: number
}

export interface TranscriptState {
  showThinking: boolean
  expandSignal: ExpandSignal
}

const defaultState: TranscriptState = {
  showThinking: false,
  expandSignal: { expand: false, nonce: 0 },
}

export const TranscriptContext = createContext<TranscriptState>(defaultState)

export function useTranscript(): TranscriptState {
  return useContext(TranscriptContext)
}
