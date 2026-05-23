import { MessageSquare } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { ChatPage } from './ChatPage'

export const route: FeatureRoute = {
  path: '/chat',
  label: 'Chat',
  icon: MessageSquare,
  order: 15,
  Component: ChatPage,
}
