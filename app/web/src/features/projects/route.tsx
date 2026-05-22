import { FolderOpen } from 'lucide-react'
import type { FeatureRoute } from '@/features/registry'
import { ProjectsPage } from './ProjectsPage'

export const route: FeatureRoute = {
  path: '/projects',
  label: 'Projects',
  icon: FolderOpen,
  order: 20,
  Component: ProjectsPage,
}
