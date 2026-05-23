import { Radio } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { PresencePage } from './PresencePage'

export const route: FeatureRoute = {
  path: '/presence',
  label: 'Presence',
  icon: Radio,
  order: 150,
  Component: PresencePage,
}
