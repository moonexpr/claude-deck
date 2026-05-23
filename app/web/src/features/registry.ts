/**
 * Feature-route registry — the parallel-enabling contract for Phase B.
 *
 * HOW TO ADD A NEW PAGE (for B2/B3/B4 page-port agents):
 *
 *   1. Create `app/web/src/features/<name>/` directory.
 *   2. Drop a `route.tsx` file that exports ONE of:
 *
 *      a) A single route (most features):
 *
 *           import type { FeatureRoute } from '@/features/registry'
 *           export const route: FeatureRoute = {
 *             path: '/your-path',
 *             label: 'Human Label',
 *             icon: SomeLucideIcon,
 *             order: 20,           // controls sidebar sort order
 *             Component: YourPage,
 *           }
 *
 *      b) Multiple routes (features with list + detail pages):
 *
 *           import type { FeatureRoute } from '@/features/registry'
 *           export const routes: FeatureRoute[] = [
 *             {
 *               path: '/sessions',
 *               label: 'Sessions',
 *               icon: TerminalIcon,
 *               order: 40,
 *               Component: SessionsListPage,
 *             },
 *             {
 *               // Detail route — registered in router, hidden from sidebar.
 *               // label/icon/order are optional on hidden routes.
 *               path: '/sessions/:projectFolder/:sessionId',
 *               hidden: true,
 *               Component: SessionDetailPage,
 *             },
 *           ]
 *
 *   3. Done. Vite's import.meta.glob picks it up automatically.
 *      You do NOT touch App.tsx, MainLayout, navigation.ts, or this file.
 *
 * INVARIANTS:
 *   - Each feature's route.tsx is self-contained. It imports only from
 *     the shared base: `@/components/ui/*`, `@/components/shared/*`,
 *     `@/lib/api`, `@/lib/constants`, `@/contexts/*`, `@/stores/*`.
 *   - Never import cross-feature: `@/features/foo` from `@/features/bar`.
 *   - `order` drives sidebar sort. Gaps are fine — leave room for insertions.
 *   - Visible routes (hidden not set or false) MUST provide label, icon, order.
 *   - Hidden routes (hidden: true) are registered in the router but omitted
 *     from the sidebar nav (used for dev/demo routes and detail pages).
 *     label, icon, and order are optional on hidden routes.
 *   - The registry sort tolerates a missing `order` (treated as 0).
 */

import type { ComponentType } from 'react'
import type { LucideIcon } from 'lucide-react'

/** A visible sidebar route — label, icon, order are required. */
export type VisibleFeatureRoute = {
  path: string
  label: string
  icon: LucideIcon
  order: number
  Component: ComponentType
  hidden?: false
}

/** A hidden route — registered in the router, omitted from the sidebar.
 *  label, icon, and order are optional. */
export type HiddenFeatureRoute = {
  path: string
  hidden: true
  Component: ComponentType
  label?: string
  icon?: LucideIcon
  order?: number
}

/** Discriminated union — narrows cleanly on `hidden`. */
export type FeatureRoute = VisibleFeatureRoute | HiddenFeatureRoute

type RouteModule =
  | { route: FeatureRoute; routes?: never }
  | { routes: FeatureRoute[]; route?: never }

// Eagerly glob all route.tsx files under features/*/
const modules = import.meta.glob<RouteModule>('./*/route.tsx', { eager: true })

export const featureRoutes: FeatureRoute[] = Object.values(modules)
  .flatMap((mod) => (mod.routes ?? (mod.route ? [mod.route] : [])))
  .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
