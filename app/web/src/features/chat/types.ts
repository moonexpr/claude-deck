/** Wire format for a single chat message — matches the server schema. */
export interface Message {
  role: 'user' | 'assistant' | 'system'
  content: string
}

/** Full conversation record returned by GET /api/v1/chat/{id} and POST /api/v1/chat. */
export interface Conversation {
  id: number
  title: string
  messages: Message[]
  created_at: string
  updated_at: string
}

/** Lightweight summary returned by GET /api/v1/chat (list). */
export interface ConversationSummary {
  id: number
  title: string
  message_count: number
  created_at: string
  updated_at: string
}

/** Response shape for GET /api/v1/chat. */
export interface ConversationListResponse {
  conversations: ConversationSummary[]
}

/** Error body returned by 400 / 403 / 404. */
export interface ErrorBody {
  status: string
  detail: string
}
