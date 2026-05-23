import { MonitorPlay } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { CCBridgePage } from './CCBridgePage'

export const route: FeatureRoute = {
  path: '/cc-bridge',
  label: 'CC Bridge',
  icon: MonitorPlay,
  order: 160,
  Component: CCBridgePage,
}
