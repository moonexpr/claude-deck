import { useEffect, useRef } from "react";
import { Outlet } from "react-router-dom";
import { LayoutDashboard } from "lucide-react";
import { useProjectStore } from "@/stores/useProjectStore";
import { useDashboardStore } from "@/stores/useDashboardStore";

/**
 * MainLayout — structural shell for app/web.
 *
 * Regions: sidebar (nav placeholder) | header | content area.
 * Visual fidelity is task B1. This shell establishes the grid
 * and wires the React Router <Outlet />.
 *
 * Store wiring (A5):
 * - Calls fetchProjects once on mount (mirrors ProjectProvider's useEffect).
 * - Re-fetches dashboard stats when the active project changes (mirrors
 *   DashboardProvider's useEffect on activeProject?.path).
 */
export function MainLayout() {
  const { activeProject, fetchProjects } = useProjectStore();
  const { stats, refreshDashboard } = useDashboardStore();

  // Fetch projects once on mount — mirrors ProjectProvider useEffect
  useEffect(() => {
    void fetchProjects();
    // fetchProjects is a stable store action reference — no re-run needed
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Re-fetch dashboard when active project changes (after initial mount)
  const prevProjectPath = useRef<string | undefined>(undefined);
  useEffect(() => {
    const currentPath = activeProject?.path;
    if (
      prevProjectPath.current !== undefined &&
      prevProjectPath.current !== currentPath
    ) {
      void refreshDashboard(currentPath);
    }
    prevProjectPath.current = currentPath;
  }, [activeProject?.path, refreshDashboard]);

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
        {/* Header — shows active project name from store (A5) */}
        <header className="flex h-14 shrink-0 items-center border-b border-[hsl(var(--border))] bg-[hsl(var(--background))] px-6">
          <span className="text-sm text-[hsl(var(--muted-foreground))]">
            {activeProject ? activeProject.name : "No active project"}
          </span>
          {stats !== null && (
            <span className="ml-auto text-xs text-[hsl(var(--muted-foreground))]">
              {stats.mcpServerCount} MCP servers
            </span>
          )}
        </header>

        {/* Content area */}
        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
