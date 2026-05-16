import { useState, useEffect, useCallback } from 'react'
import { useSearchParams } from 'react-router-dom'
import { BarChart3 } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { RefreshButton } from '@/components/shared/RefreshButton'
import { PageHeader } from '@/components/layout/PageHeader'
import { useUsageApi } from '@/hooks/useUsageApi'
import { useProjectContext } from '@/contexts/ProjectContext'
import { UsageSummaryCards } from './UsageSummaryCards'
import { DailyUsageChart } from './DailyUsageChart'
import { CostChart } from './CostChart'
import { SessionUsageTable } from './SessionUsageTable'
import { BlocksView } from './BlocksView'
import { MonthlyUsageChart } from './MonthlyUsageChart'
import { ExportCard } from './ExportCard'
import { getFromCache, saveToCache, isCacheStale, invalidateCache } from '@/lib/usageCache'
import type {
  UsageSummary,
  DailyUsage,
  SessionUsage,
  MonthlyUsage,
  SessionBlock,
} from '@/types/usage'

interface UsageCacheData {
  summary: UsageSummary | null
  daily: DailyUsage[]
  sessions: SessionUsage[]
  monthly: MonthlyUsage[]
  blocks: SessionBlock[]
  activeBlock?: SessionBlock
  totalSessionCost: number
  totalBlockCost: number
}

export function UsagePage() {
  const [searchParams, setSearchParams] = useSearchParams()
  const { projects, activeProject } = useProjectContext()
  const { getSummary, getDaily, getSessions, getMonthly, getBlocks } = useUsageApi()

  const [selectedProject, setSelectedProject] = useState<string | null>(
    activeProject?.path || searchParams.get('project') || null
  )
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // Data states
  const [summary, setSummary] = useState<UsageSummary | null>(null)
  const [dailyData, setDailyData] = useState<DailyUsage[]>([])
  const [sessionData, setSessionData] = useState<SessionUsage[]>([])
  const [monthlyData, setMonthlyData] = useState<MonthlyUsage[]>([])
  const [blocks, setBlocks] = useState<SessionBlock[]>([])
  const [activeBlock, setActiveBlock] = useState<SessionBlock | undefined>()
  const [totalSessionCost, setTotalSessionCost] = useState(0)
  const [totalBlockCost, setTotalBlockCost] = useState(0)

  const loadData = useCallback(async (forceRefresh = false) => {
    setError(null)

    // Try to load from cache first (instant)
    if (!forceRefresh) {
      const cached = getFromCache<UsageCacheData>(selectedProject)
      if (cached) {
        // Apply cached data immediately (no loading spinner)
        setSummary(cached.summary)
        setDailyData(cached.daily)
        setSessionData(cached.sessions)
        setTotalSessionCost(cached.totalSessionCost)
        setMonthlyData(cached.monthly)
        setBlocks(cached.blocks)
        setActiveBlock(cached.activeBlock)
        setTotalBlockCost(cached.totalBlockCost)
        setLoading(false)

        // If cache is fresh, we're done
        if (!isCacheStale(selectedProject)) return
        // Otherwise, continue to background refresh (don't show loading)
      }
    }

    // Show loading only if no cached data or force refresh
    const hasCachedData = !forceRefresh && getFromCache<UsageCacheData>(selectedProject)
    if (!hasCachedData) setLoading(true)

    try {
      const params = selectedProject ? { project_path: selectedProject } : undefined

      // Fetch all data in parallel
      const [summaryRes, dailyRes, sessionRes, monthlyRes, blockRes] = await Promise.all([
        getSummary(params),
        getDaily(params),
        getSessions({ ...params, limit: 100 }),
        getMonthly(params),
        getBlocks({ ...params, recent: true }),
      ])

      // Update state
      setSummary(summaryRes.summary)
      setDailyData(dailyRes.data)
      setSessionData(sessionRes.data)
      setTotalSessionCost(sessionRes.total_cost)
      setMonthlyData(monthlyRes.data)
      setBlocks(blockRes.data)
      setActiveBlock(blockRes.active_block)
      setTotalBlockCost(blockRes.total_cost)

      // Save to cache
      saveToCache<UsageCacheData>(selectedProject, {
        summary: summaryRes.summary,
        daily: dailyRes.data,
        sessions: sessionRes.data,
        monthly: monthlyRes.data,
        blocks: blockRes.data,
        activeBlock: blockRes.active_block,
        totalSessionCost: sessionRes.total_cost,
        totalBlockCost: blockRes.total_cost,
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load usage data')
    } finally {
      setLoading(false)
    }
  }, [selectedProject, getSummary, getDaily, getSessions, getMonthly, getBlocks])

  const handleRefresh = useCallback(() => {
    invalidateCache(selectedProject)
    loadData(true)
  }, [loadData, selectedProject])

  useEffect(() => {
    loadData()
  }, [loadData])

  const handleProjectChange = (value: string) => {
    const path = value === 'all' ? null : value
    setSelectedProject(path)
    setSearchParams(path ? { project: path } : {})
  }

  return (
    <div className="space-y-4 md:space-y-6">
      {/* Header */}
      <PageHeader
        title="Usage Tracking"
        description="Monitor your Claude Code token usage and costs"
        icon={BarChart3}
      >
        <RefreshButton onClick={handleRefresh} loading={loading} />
      </PageHeader>

      {/* Error Display */}
      {error && (
        <Card className="border-destructive">
          <CardHeader className="p-4">
            <CardTitle className="text-destructive text-lg">Error</CardTitle>
          </CardHeader>
          <CardContent className="px-4 pb-4">
            <p className="text-sm">{error}</p>
          </CardContent>
        </Card>
      )}

      {/* Project Filter */}
      <Card>
        <CardHeader className="pb-3 md:pb-6 p-4 md:p-6">
          <CardTitle className="text-lg md:text-xl">Filter</CardTitle>
          <CardDescription className="text-xs md:text-sm">Filter usage data by project</CardDescription>
        </CardHeader>
        <CardContent className="px-4 pb-4 md:px-6 md:pb-6">
          <Select value={selectedProject || 'all'} onValueChange={handleProjectChange}>
            <SelectTrigger className="w-full sm:w-[300px]">
              <SelectValue placeholder="All projects" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All Projects</SelectItem>
              {projects.map(p => (
                <SelectItem key={p.path} value={p.path}>
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardContent>
      </Card>

      {/* Export */}
      <ExportCard projectPath={selectedProject} />

      {/* Summary Cards */}
      <UsageSummaryCards summary={summary} loading={loading} />

      {/* Tabbed Views */}
      <Tabs defaultValue="overview" className="space-y-4">
        <div className="overflow-x-auto pb-2 -mb-2">
          <TabsList className="w-full justify-start sm:justify-center">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="daily">Daily</TabsTrigger>
            <TabsTrigger value="sessions">Sessions</TabsTrigger>
            <TabsTrigger value="monthly">Monthly</TabsTrigger>
            <TabsTrigger value="blocks">
              Blocks
              {activeBlock && (
                <span className="ml-1 w-2 h-2 bg-success rounded-full animate-pulse" />
              )}
            </TabsTrigger>
          </TabsList>
        </div>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-4">
          <div className="grid gap-4 lg:grid-cols-2">
            <CostChart data={dailyData} loading={loading} />
            <MonthlyUsageChart data={monthlyData} loading={loading} />
          </div>
          <DailyUsageChart data={dailyData.slice(0, 14)} loading={loading} />
        </TabsContent>

        {/* Daily Tab */}
        <TabsContent value="daily" className="space-y-4">
          <DailyUsageChart data={dailyData} loading={loading} />
          <CostChart data={dailyData} loading={loading} />
        </TabsContent>

        {/* Sessions Tab */}
        <TabsContent value="sessions">
          <SessionUsageTable
            data={sessionData}
            loading={loading}
            totalCost={totalSessionCost}
          />
        </TabsContent>

        {/* Monthly Tab */}
        <TabsContent value="monthly">
          <MonthlyUsageChart data={monthlyData} loading={loading} />
        </TabsContent>

        {/* Blocks Tab */}
        <TabsContent value="blocks">
          <BlocksView
            blocks={blocks}
            activeBlock={activeBlock}
            loading={loading}
            totalCost={totalBlockCost}
          />
        </TabsContent>
      </Tabs>
    </div>
  )
}
