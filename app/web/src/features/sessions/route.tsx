import { MessageSquare } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { SessionsPage } from './SessionsPage'
import { SessionViewPage } from './SessionViewPage'

export const routes: FeatureRoute[] = [
  {
    path: '/sessions',
    label: 'Sessions',
    icon: MessageSquare,
    order: 140,
    Component: SessionsPage,
  },
  {
    path: '/sessions/:projectFolder/:sessionId',
    hidden: true,
    Component: SessionViewPage,
  },
]
