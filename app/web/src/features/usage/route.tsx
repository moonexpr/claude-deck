import { BarChart3 } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { UsagePage } from './UsagePage'

export const route: FeatureRoute = {
  path: '/usage',
  label: 'Usage',
  icon: BarChart3,
  order: 190,
  Component: UsagePage,
}
