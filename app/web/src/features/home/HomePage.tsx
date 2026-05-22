import { useEffect } from 'react'
import { LayoutDashboard } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { RefreshButton } from '@/components/shared/RefreshButton'
import { Progress } from '@/components/ui/progress'
import { PageHeader } from '@/components/layout/PageHeader'
import { useNavigate } from 'react-router-dom'
import { useDashboardStore } from '@/stores/useDashboardStore'
import { useProjectStore } from '@/stores/useProjectStore'

/** Get relative time string (e.g., "2 hours ago") */
function getRelativeTime(isoString: string): string {
  const date = new Date(isoString)
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  if (days > 0) return `${days}d ago`
  if (hours > 0) return `${hours}h ago`
  if (minutes > 0) return `${minutes}m ago`
  return 'just now'
}

export function HomePage() {
  const { stats, loading, error, lastFetched, refreshDashboard } = useDashboardStore()
  const { projects, activeProject } = useProjectStore()
  const navigate = useNavigate()

  useEffect(() => {
    refreshDashboard(activeProject?.path)
  }, [refreshDashboard, activeProject?.path])

  return (
    <div className="space-y-4 md:space-y-6">
      <PageHeader
        title="Dashboard"
        description="Overview of your Claude Code configuration"
        icon={LayoutDashboard}
      >
        {lastFetched && (
          <span className="text-[10px] md:text-xs text-muted-foreground">
            Updated {getRelativeTime(lastFetched.toISOString())}
          </span>
        )}
        <RefreshButton onClick={() => refreshDashboard(activeProject?.path)} loading={loading} />
      </PageHeader>

      {error && (
        <Card className="border-destructive">
          <CardHeader>
            <CardTitle className="text-destructive">Error</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm">{error}</p>
          </CardContent>
        </Card>
      )}

      {loading && !stats && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {[...Array(9)].map((_, i) => (
            <Card key={i}>
              <CardHeader className="pb-2">
                <CardDescription>Loading...</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">-</div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {stats && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {/* Tier 1: Overview & Setup */}
          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Projects</CardDescription>
              <CardTitle className="text-3xl">{projects.length}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Tracked projects</p>
            </CardContent>
          </Card>

          {/* Tier 2: Core Configuration */}
          <Card>
            <CardHeader className="pb-2">
              <CardDescription>MCP Servers</CardDescription>
              <CardTitle className="text-3xl">{stats.mcpServerCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Configured MCP servers</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Commands</CardDescription>
              <CardTitle className="text-3xl">{stats.commandCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Slash commands available</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Plugins</CardDescription>
              <CardTitle className="text-3xl">{stats.pluginCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Installed plugins</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Hooks</CardDescription>
              <CardTitle className="text-3xl">{stats.hookCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Automation hooks configured</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Permissions</CardDescription>
              <CardTitle className="text-3xl">{stats.permissionCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Permission rules</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Agents</CardDescription>
              <CardTitle className="text-3xl">{stats.agentCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Custom agents (user, project, plugin)</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Skills</CardDescription>
              <CardTitle className="text-3xl">{stats.skillCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Available skills</p>
            </CardContent>
          </Card>

          {/* Tier 3: Customization */}
          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Output Styles</CardDescription>
              <CardTitle className="text-3xl">{stats.outputStyleCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Custom output formats</p>
            </CardContent>
          </Card>

          {/* Tier 4: Monitoring */}
          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Sessions</CardDescription>
              <CardTitle className="text-3xl">{stats.sessionCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-xs text-muted-foreground space-y-1">
                <p>{stats.sessionsToday} today</p>
                <p>{stats.sessionsThisWeek} this week</p>
                {stats.mostActiveProject && (
                  <p className="text-primary">Most active: {stats.mostActiveProject}</p>
                )}
              </div>
              <Button
                variant="link"
                className="p-0 h-auto mt-2"
                onClick={() => navigate('/sessions')}
              >
                View all sessions →
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Plans</CardDescription>
              <CardTitle className="text-3xl">{stats.planCount}</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">Execution plans</p>
              <Button
                variant="link"
                className="p-0 h-auto mt-2"
                onClick={() => navigate('/plans')}
              >
                View all plans →
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardDescription>Context Window</CardDescription>
              <CardTitle className="text-3xl">
                {stats.contextActiveCount > 0 ? `${stats.contextHighestPct.toFixed(0)}%` : '--'}
              </CardTitle>
            </CardHeader>
            <CardContent>
              {stats.contextActiveCount > 0 ? (
                <div className="space-y-2">
                  <Progress
                    value={stats.contextHighestPct}
                    className={`h-2 ${
                      stats.contextHighestPct >= 95 ? '[&>div]:bg-red-500' :
                      stats.contextHighestPct >= 80 ? '[&>div]:bg-orange-500' :
                      stats.contextHighestPct >= 50 ? '[&>div]:bg-yellow-500' :
                      '[&>div]:bg-green-500'
                    }`}
                  />
                  <p className="text-xs text-muted-foreground">
                    {stats.contextActiveCount} active session{stats.contextActiveCount !== 1 ? 's' : ''}
                    {stats.contextHighestProject && ` - ${stats.contextHighestProject}`}
                  </p>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground">No active sessions</p>
              )}
              <Button
                variant="link"
                className="p-0 h-auto mt-2"
                onClick={() => navigate('/context')}
              >
                View context →
              </Button>
            </CardContent>
          </Card>
        </div>
      )}

      {stats && (
        <Card>
          <CardHeader>
            <CardTitle>Quick Status</CardTitle>
            <CardDescription>Configuration health indicators</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-2">
              <div className="flex items-center justify-between text-sm">
                <span>Settings keys:</span>
                <span className="font-medium">{stats.settingsKeys} configured</span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span>Allow rules:</span>
                <span className="font-medium text-green-600 dark:text-green-400">{stats.allowRules}</span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span>Deny rules:</span>
                <span className="font-medium text-destructive">{stats.denyRules}</span>
              </div>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
