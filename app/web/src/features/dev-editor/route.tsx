import { Code2 } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { CodeEditorDemoPage } from './CodeEditorDemoPage'

export const route: FeatureRoute = {
  path: '/dev/editor',
  label: 'Editor Demo',
  icon: Code2,
  order: 900,
  Component: CodeEditorDemoPage,
  hidden: true,
}
