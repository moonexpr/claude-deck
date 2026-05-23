import { Activity } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { StatusLinePage } from './StatusLinePage'

export const route: FeatureRoute = {
  path: '/statusline',
  label: 'Status Line',
  icon: Activity,
  order: 130,
  Component: StatusLinePage,
}
