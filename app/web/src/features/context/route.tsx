import { Gauge } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { ContextPage } from './ContextPage'

export const route: FeatureRoute = {
  path: '/context',
  label: 'Context',
  icon: Gauge,
  order: 180,
  Component: ContextPage,
}
