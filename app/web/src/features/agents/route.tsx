import { Bot } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { AgentsPage } from './AgentsPage'

export const route: FeatureRoute = {
  path: '/agents',
  label: 'Agents',
  icon: Bot,
  order: 90,
  Component: AgentsPage,
}
