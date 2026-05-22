import { Brain } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { MemoryPage } from './MemoryPage'

export const route: FeatureRoute = {
  path: '/memory',
  label: 'Memory',
  icon: Brain,
  order: 110,
  Component: MemoryPage,
}
