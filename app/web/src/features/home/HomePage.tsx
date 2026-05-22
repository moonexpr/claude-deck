import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { LayoutDashboard } from "lucide-react";

/**
 * Scaffold placeholder — A4.
 * Real dashboard content is task B1.
 */
export function HomePage() {
  return (
    <div className="flex flex-col items-center justify-center min-h-full gap-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <LayoutDashboard className="h-5 w-5 text-[hsl(var(--primary))]" />
            Claude Deck — app/web scaffold (A4)
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-sm text-[hsl(var(--muted-foreground))]">
            Vite 7 + React 19 + TypeScript 5.9 + Tailwind v4 + shadcn/ui.
            Feature pages are task B1.
          </p>
          <Button variant="outline" size="sm">
            Scaffold live
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
