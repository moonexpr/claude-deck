import { useState, useEffect, useRef } from 'react'
import { Outlet } from 'react-router-dom'
import { Header } from './Header'
import { Sidebar } from './Sidebar'
import { MobileSidebar } from './MobileSidebar'
import { Footer } from './Footer'
import { SidebarContext } from '@/contexts/SidebarContext'
import { useProjectStore } from '@/stores/useProjectStore'
import { useDashboardStore } from '@/stores/useDashboardStore'

export function MainLayout() {
  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)

  // Keep store wiring from A5 — fetch projects on mount, refresh dashboard
  // when active project changes.
  const { activeProject, fetchProjects } = useProjectStore()
  const { refreshDashboard } = useDashboardStore()

  useEffect(() => {
    void fetchProjects()
    // fetchProjects is a stable store action reference
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const prevProjectPath = useRef<string | undefined>(undefined)
  useEffect(() => {
    const currentPath = activeProject?.path
    if (
      prevProjectPath.current !== undefined &&
      prevProjectPath.current !== currentPath
    ) {
      void refreshDashboard(currentPath)
    }
    prevProjectPath.current = currentPath
  }, [activeProject?.path, refreshDashboard])

  return (
    <SidebarContext.Provider value={{ collapsed, setCollapsed, mobileOpen, setMobileOpen }}>
      <div className="flex h-screen flex-col bg-gradient-brand">
        <Header />
        <div className="flex flex-1 overflow-hidden relative">
          <Sidebar />
          <MobileSidebar />
          <main className="flex-1 overflow-y-auto p-4 md:p-6">
            <Outlet />
          </main>
        </div>
        <Footer />
      </div>
    </SidebarContext.Provider>
  )
}
