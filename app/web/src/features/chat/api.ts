import { apiClient } from '@/lib/api'
import type {
  Conversation,
  ConversationListResponse,
  Message,
} from './types'

const BASE = 'chat'

/** GET /api/v1/chat — list all conversations ordered by updated_at DESC. */
export async function listConversations(): Promise<ConversationListResponse> {
  return apiClient<ConversationListResponse>(BASE)
}

/** GET /api/v1/chat/{id} — fetch a single conversation with messages. */
export async function getConversation(id: number): Promise<Conversation> {
  return apiClient<Conversation>(`${BASE}/${id}`)
}

/** POST /api/v1/chat — create a new conversation. */
export async function createConversation(
  title: string,
  messages: Message[] = [],
): Promise<Conversation> {
  return apiClient<Conversation>(BASE, {
    method: 'POST',
    body: JSON.stringify({ title, messages }),
  })
}

/** PUT /api/v1/chat/{id} — update title and/or messages. */
export async function updateConversation(
  id: number,
  patch: { title?: string; messages?: Message[] },
): Promise<Conversation> {
  return apiClient<Conversation>(`${BASE}/${id}`, {
    method: 'PUT',
    body: JSON.stringify(patch),
  })
}

/** DELETE /api/v1/chat/{id} — delete a conversation. Returns 204 No Content. */
export async function deleteConversation(id: number): Promise<void> {
  await apiClient<void>(`${BASE}/${id}`, { method: 'DELETE' })
}
