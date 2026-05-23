import { LayoutDashboard } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { HomePage } from './HomePage'

export const route: FeatureRoute = {
  path: '/',
  label: 'Dashboard',
  icon: LayoutDashboard,
  order: 10,
  Component: HomePage,
}
