import { Archive } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { BackupPage } from './BackupPage'

export const route: FeatureRoute = {
  path: '/backup',
  label: 'Backup',
  icon: Archive,
  order: 200,
  Component: BackupPage,
}
