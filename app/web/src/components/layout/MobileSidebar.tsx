import { NavLink } from 'react-router-dom'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { ProjectSwitcher } from '@/components/layout/ProjectSwitcher'
import { useSidebar } from '@/contexts/SidebarContext'
import { featureRoutes } from '@/features/registry'
import {
  Dialog,
  DialogContent,
  DialogPortal,
  DialogOverlay,
  DialogTitle,
} from '@/components/ui/dialog'

export function MobileSidebar() {
  const { mobileOpen, setMobileOpen } = useSidebar()

  const navRoutes = featureRoutes.filter((r) => !r.hidden)

  return (
    <Dialog open={mobileOpen} onOpenChange={setMobileOpen}>
      <DialogPortal>
        <DialogOverlay />
        <DialogContent
          className="fixed inset-y-0 left-0 z-50 h-full w-[280px] border-r bg-background p-0 shadow-lg transition ease-in-out data-[state=closed]:duration-300 data-[state=open]:duration-500 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:slide-out-to-left data-[state=open]:slide-in-from-left"
        >
          <DialogTitle className="sr-only">Navigation Menu</DialogTitle>

          <div className="flex h-full flex-col">
            <div className="flex items-center justify-between p-4 border-b">
              <span className="font-bold text-lg text-primary">Claude Deck</span>
              <button
                onClick={() => setMobileOpen(false)}
                className="p-1 rounded-md hover:bg-accent transition-colors"
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="py-4 border-b">
              <ProjectSwitcher />
            </div>

            <nav className="flex-1 overflow-y-auto p-4 flex flex-col gap-1">
              {navRoutes.map((item) => (
                <NavLink
                  key={item.path}
                  to={item.path}
                  end={item.path === '/'}
                  onClick={() => setMobileOpen(false)}
                  className={({ isActive }) =>
                    cn(
                      'flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                      isActive
                        ? 'bg-primary text-primary-foreground'
                        : 'text-foreground hover:bg-accent hover:text-accent-foreground'
                    )
                  }
                >
                  <item.icon className="h-4 w-4 shrink-0" />
                  {item.label}
                </NavLink>
              ))}
            </nav>
          </div>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  )
}
