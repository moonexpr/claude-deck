import { Terminal } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { CommandsPage } from './CommandsPage'

export const route: FeatureRoute = {
  path: '/commands',
  label: 'Commands',
  icon: Terminal,
  order: 50,
  Component: CommandsPage,
}
