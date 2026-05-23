import { ClipboardList } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { PlansPage } from './PlansPage'
import { PlanDetailPage } from './PlanDetailPage'

export const routes: FeatureRoute[] = [
  {
    path: '/plans',
    label: 'Plans',
    icon: ClipboardList,
    order: 170,
    Component: PlansPage,
  },
  {
    path: '/plans/:filename',
    hidden: true,
    Component: PlanDetailPage,
  },
]
