import { Bot } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { AiDemoPage } from './AiDemoPage'

export const route: FeatureRoute = {
  path: '/dev/ai',
  label: 'AI Demo',
  icon: Bot,
  order: 901,
  Component: AiDemoPage,
  hidden: true,
}
