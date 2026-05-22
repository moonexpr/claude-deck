import { Package } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { PluginsPage } from './PluginsPage'

export const route: FeatureRoute = {
  path: '/plugins',
  label: 'Plugins',
  icon: Package,
  order: 60,
  Component: PluginsPage,
}
