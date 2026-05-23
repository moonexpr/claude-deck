import * as DialogPrimitive from '@radix-ui/react-dialog'
import { NavLink } from 'react-router-dom'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { ProjectSwitcher } from '@/components/layout/ProjectSwitcher'
import { useSidebar } from '@/contexts/SidebarContext'
import { featureRoutes } from '@/features/registry'
import { Dialog, DialogPortal, DialogOverlay, DialogTitle } from '@/components/ui/dialog'

export function MobileSidebar() {
  const { mobileOpen, setMobileOpen } = useSidebar()

  const navRoutes = featureRoutes.filter((r) => !r.hidden)

  return (
    <Dialog open={mobileOpen} onOpenChange={setMobileOpen}>
      <DialogPortal>
        <DialogOverlay />
        {/*
          Use DialogPrimitive.Content directly — not shadcn DialogContent.
          Shadcn's DialogContent bakes in: its own portal+overlay (double-wrapping),
          its own absolute X close button (duplicate), display:grid + centering
          transforms (breaks the flex height chain so the nav cannot scroll).
          Here we own the Content's styling entirely.
        */}
        <DialogPrimitive.Content
          className={cn(
            // Drawer: pinned left, full viewport height, flex column
            'fixed inset-y-0 left-0 z-50 flex h-full w-[280px] flex-col',
            'border-r bg-background shadow-lg',
            // Slide animation — in from left, out to left
            'transition ease-in-out',
            'data-[state=open]:duration-300 data-[state=closed]:duration-200',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=open]:slide-in-from-left data-[state=closed]:slide-out-to-left',
          )}
        >
          {/* Accessibility: sr-only title satisfies Radix's DialogTitle requirement */}
          <DialogTitle className="sr-only">Navigation Menu</DialogTitle>

          {/* Header — title + single close button */}
          <div className="flex shrink-0 items-center justify-between border-b p-4">
            <span className="text-lg font-bold text-primary">Claude Deck</span>
            <button
              onClick={() => setMobileOpen(false)}
              aria-label="Close navigation menu"
              className="rounded-md p-1 transition-colors hover:bg-accent"
            >
              <X className="h-5 w-5" />
            </button>
          </div>

          {/* Project switcher */}
          <div className="shrink-0 border-b py-4">
            <ProjectSwitcher />
          </div>

          {/*
            Scrollable nav.
            flex-1 makes it take remaining height.
            min-h-0 is the standard flex fix — without it a flex child's
            overflow:auto is ignored because the implicit min-height is
            "content size", so the browser never constrains the height and
            the overflow never fires.
          */}
          <nav
            aria-label="Site navigation"
            className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-3"
          >
            {navRoutes.map((item) => (
              <NavLink
                key={item.path}
                to={item.path}
                end={item.path === '/'}
                onClick={() => setMobileOpen(false)}
                className={({ isActive }) =>
                  cn(
                    // Distinct tappable button look: border, comfortable touch
                    // height (min-h-[44px] meets Apple's 44pt guideline), gap
                    // between items comes from the parent gap-1.
                    'flex min-h-[44px] items-center gap-3 rounded-md border px-3 py-2 text-sm font-medium transition-colors',
                    isActive
                      ? 'border-primary bg-primary text-primary-foreground'
                      : 'border-border text-foreground hover:border-primary/40 hover:bg-accent hover:text-accent-foreground',
                  )
                }
              >
                <item.icon className="h-4 w-4 shrink-0" />
                {item.label}
              </NavLink>
            ))}
          </nav>
        </DialogPrimitive.Content>
      </DialogPortal>
    </Dialog>
  )
}
