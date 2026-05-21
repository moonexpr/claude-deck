import { Outlet } from "react-router-dom";
import { LayoutDashboard } from "lucide-react";

/**
 * MainLayout — structural shell for app/web.
 *
 * Regions: sidebar (nav placeholder) | header | content area.
 * Visual fidelity is task B1. This shell establishes the grid
 * and wires the React Router <Outlet />.
 */
export function MainLayout() {
  return (
    <div className="flex h-screen bg-[hsl(var(--background))] text-[hsl(var(--foreground))]">
      {/* Sidebar */}
      <aside className="flex w-56 shrink-0 flex-col border-r border-[hsl(var(--border))] bg-[hsl(var(--card))]">
        {/* Brand mark */}
        <div className="flex h-14 items-center gap-2 border-b border-[hsl(var(--border))] px-4">
          <LayoutDashboard className="h-5 w-5 text-[hsl(var(--primary))]" />
          <span className="font-semibold tracking-tight">Claude Deck</span>
        </div>

        {/* Nav placeholder — populated in task B1 */}
        <nav className="flex-1 overflow-y-auto px-3 py-4">
          <p className="px-2 text-xs text-[hsl(var(--muted-foreground))]">
            Navigation — task B1
          </p>
        </nav>
      </aside>

      {/* Main column */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Header */}
        <header className="flex h-14 shrink-0 items-center border-b border-[hsl(var(--border))] bg-[hsl(var(--background))] px-6">
          <span className="text-sm text-[hsl(var(--muted-foreground))]">
            Header — task B1
          </span>
        </header>

        {/* Content area */}
        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
