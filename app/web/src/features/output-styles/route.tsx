import { Paintbrush } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { OutputStylesPage } from './OutputStylesPage'

export const route: FeatureRoute = {
  path: '/output-styles',
  label: 'Output Styles',
  icon: Paintbrush,
  order: 120,
  Component: OutputStylesPage,
}
