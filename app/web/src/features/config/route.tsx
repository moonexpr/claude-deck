import { Settings } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { ConfigViewerPage } from './ConfigViewerPage'

export const route: FeatureRoute = {
  path: '/config',
  label: 'Config',
  icon: Settings,
  order: 30,
  Component: ConfigViewerPage,
}
