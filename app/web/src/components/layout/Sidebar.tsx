import { NavLink } from 'react-router-dom'
import { cn } from '@/lib/utils'
import { ProjectSwitcher } from '@/components/layout/ProjectSwitcher'
import { useSidebar } from '@/contexts/SidebarContext'
import { featureRoutes } from '@/features/registry'
import {
  PanelLeftClose,
  PanelLeftOpen,
} from 'lucide-react'

export function Sidebar() {
  const { collapsed, setCollapsed } = useSidebar()

  const navRoutes = featureRoutes.filter((r) => !r.hidden)

  return (
    <aside className={cn(
      'hidden lg:flex border-r bg-background transition-all duration-200 flex-col',
      collapsed ? 'w-14' : 'w-64'
    )}>
      {!collapsed && (
        <div className="py-4 border-b">
          <ProjectSwitcher />
        </div>
      )}
      <nav className={cn(
        'flex flex-col gap-1 flex-1 overflow-y-auto',
        collapsed ? 'p-2' : 'p-4'
      )}>
        {navRoutes.map((item) => (
          <NavLink
            key={item.path}
            to={item.path}
            end={item.path === '/'}
            title={collapsed ? item.label : undefined}
            className={({ isActive }) =>
              cn(
                'flex items-center rounded-md text-sm font-medium transition-colors',
                collapsed ? 'justify-center p-2' : 'gap-2 px-3 py-2',
                isActive
                  ? 'bg-primary text-primary-foreground'
                  : 'text-foreground hover:bg-accent hover:text-accent-foreground'
              )
            }
          >
            <item.icon className="h-4 w-4 shrink-0" />
            {!collapsed && item.label}
          </NavLink>
        ))}
      </nav>
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="flex items-center justify-center p-3 border-t text-muted-foreground hover:text-foreground transition-colors"
        title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        {collapsed ? <PanelLeftOpen className="h-4 w-4" /> : <PanelLeftClose className="h-4 w-4" />}
      </button>
    </aside>
  )
}
