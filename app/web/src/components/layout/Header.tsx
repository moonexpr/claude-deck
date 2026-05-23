import { Terminal, Radio, Menu } from "lucide-react";
import { ThemeToggle } from "@/components/ui/theme-toggle";
import { Badge } from "@/components/ui/badge";
import { useTheme } from "@/contexts/ThemeContext";
import { useSystemStatus } from "@/hooks/useSystemStatus";
import { useSidebar } from "@/contexts/SidebarContext";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

export function Header() {
  const { theme } = useTheme();
  const status = useSystemStatus();
  const { setMobileOpen } = useSidebar();

  return (
    <header className="border-b bg-background sticky top-0 z-50">
      <div className="flex h-16 items-center justify-between px-4 md:px-6">
        <div className="flex items-center gap-3">
          <Button
            variant="ghost"
            size="icon"
            className="lg:hidden"
            onClick={() => setMobileOpen(true)}
          >
            <Menu className="h-6 w-6" />
          </Button>
          <img
            src={theme === "light" ? "/logo-light.png" : "/logo-dark.png"}
            alt="Claude Deck"
            className="h-8 w-8 md:h-10 md:w-10"
          />
          <div className="hidden sm:block">
            <h1 className="text-lg md:text-2xl font-bold text-primary leading-tight">Claude Deck</h1>
            <p className="text-[10px] md:text-xs text-muted-foreground">Your Claude Code command centre</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {status && (
            <>
              {status.claudeCodeVersion && (
                <Badge variant="outline" className="hidden sm:flex gap-1 font-mono text-[10px] md:text-xs">
                  <Terminal className="h-3 w-3" />
                  v{status.claudeCodeVersion}
                </Badge>
              )}
              <Badge
                variant="secondary"
                className={cn(
                  "gap-1 text-[10px] md:text-xs",
                  status.activeSessions > 0 && "bg-green-500/15 text-green-600 dark:text-green-400"
                )}
              >
                <Radio className="h-3 w-3" />
                {status.activeSessions} active
              </Badge>
            </>
          )}
          <ThemeToggle />
        </div>
      </div>
    </header>
  )
}
