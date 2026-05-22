import { Shield } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { PermissionsPage } from './PermissionsPage'

export const route: FeatureRoute = {
  path: '/permissions',
  label: 'Permissions',
  icon: Shield,
  order: 80,
  Component: PermissionsPage,
}
