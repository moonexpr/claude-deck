import { Sparkles } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { SkillsPage } from './SkillsPage'

export const route: FeatureRoute = {
  path: '/skills',
  label: 'Skills',
  icon: Sparkles,
  order: 100,
  Component: SkillsPage,
}
