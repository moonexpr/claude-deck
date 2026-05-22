import { useEffect, useState, useCallback, useRef } from 'react'
import { Gauge, HelpCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { RefreshButton } from '@/components/shared/RefreshButton'
import { PageHeader } from '@/components/layout/PageHeader'
import { cn } from '@/lib/utils'
import { useContextApi } from './useContextApi'
import { ActiveSessionsList } from './ActiveSessionsList'
import { ContextGauge } from './ContextGauge'
import { ContextTimelineChart } from './ContextTimelineChart'
import { ContextCompositionChart } from './ContextCompositionChart'
import { FileConsumptionTable } from './FileConsumptionTable'
import { ToolUsageTable } from './ToolUsageTable'
import { CacheEfficiencyCard } from './CacheEfficiencyCard'
import { ProjectionsCard } from './ProjectionsCard'
import type { ActiveSessionContext, ContextAnalysis } from '@/types/context'

export function ContextPage() {
  const { getActiveSessions, getSessionContext } = useContextApi()
  const [sessions, setSessions] = useState<ActiveSessionContext[]>([])
  const [selectedSession, setSelectedSession] = useState<ActiveSessionContext | null>(null)
  const [analysis, setAnalysis] = useState<ContextAnalysis | null>(null)
  const [loading, setLoading] = useState(true)
  const [analysisLoading, setAnalysisLoading] = useState(false)
  const [showHelp, setShowHelp] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchSessions = useCallback(async () => {
    try {
      const data = await getActiveSessions()
      setSessions(data.sessions)
    } catch {
      // Silently handle polling errors
    } finally {
      setLoading(false)
    }
  }, [getActiveSessions])

  const fetchAnalysis = useCallback(async (session: ActiveSessionContext) => {
    setAnalysisLoading(true)
    try {
      const data = await getSessionContext(session.project_folder, session.session_id)
      setAnalysis(data.analysis)
    } catch {
      setAnalysis(null)
    } finally {
      setAnalysisLoading(false)
    }
  }, [getSessionContext])

  const handleSelect = useCallback((session: ActiveSessionContext) => {
    setSelectedSession(session)
    fetchAnalysis(session)
  }, [fetchAnalysis])

  const handleRefresh = useCallback(async () => {
    setLoading(true)
    await fetchSessions()
    if (selectedSession) {
      await fetchAnalysis(selectedSession)
    }
  }, [fetchSessions, fetchAnalysis, selectedSession])

  // Initial fetch + auto-poll
  useEffect(() => {
    fetchSessions()

    const startPolling = () => {
      intervalRef.current = setInterval(fetchSessions, 10_000)
    }

    const handleVisibility = () => {
      if (document.hidden) {
        if (intervalRef.current) clearInterval(intervalRef.current)
      } else {
        fetchSessions()
        startPolling()
      }
    }

    startPolling()
    document.addEventListener('visibilitychange', handleVisibility)

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current)
      document.removeEventListener('visibilitychange', handleVisibility)
    }
  }, [fetchSessions])

  return (
    <div className="space-y-4 md:space-y-6">
      <PageHeader
        title="Context Window"
        description="Analyze context window usage across active sessions"
        icon={Gauge}
      >
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setShowHelp(!showHelp)}
          className={cn("h-8 md:h-10 text-xs md:text-sm", showHelp && "text-primary")}
        >
          <HelpCircle className="h-3.5 w-3.5 md:h-4 md:w-4 mr-1 md:mr-2" />
          Help
        </Button>
        <RefreshButton onClick={handleRefresh} loading={loading} />
      </PageHeader>

      {/* Active Sessions */}
      <ActiveSessionsList
        sessions={sessions}
        selectedId={selectedSession?.session_id}
        onSelect={handleSelect}
      />

      {/* Analysis */}
      {analysisLoading && (
        <div className="py-8 text-center">
          <p className="text-muted-foreground">Analyzing session...</p>
        </div>
      )}

      {analysis && !analysisLoading && (
        <div className="space-y-4">
          {/* Top row: gauge + projections + cache */}
          <div className="grid gap-4 md:grid-cols-3">
            <div className="flex items-center justify-center">
              <ContextGauge
                percentage={analysis.context_percentage}
                currentTokens={analysis.current_context_tokens}
                maxTokens={analysis.max_context_tokens}
                model={analysis.model}
                showHelp={showHelp}
              />
            </div>
            <ProjectionsCard
              avgTokensPerTurn={analysis.avg_tokens_per_turn}
              estimatedTurnsRemaining={analysis.estimated_turns_remaining}
              contextZone={analysis.context_zone}
              totalTurns={analysis.total_turns}
              showHelp={showHelp}
            />
            <CacheEfficiencyCard cache={analysis.cache_efficiency} showHelp={showHelp} />
          </div>

          {/* Charts */}
          <ContextTimelineChart
            snapshots={analysis.snapshots}
            maxTokens={analysis.max_context_tokens}
            showHelp={showHelp}
          />

          <div className="grid gap-4 md:grid-cols-2">
            <ContextCompositionChart composition={analysis.composition} showHelp={showHelp} />
            <FileConsumptionTable files={analysis.file_consumptions} showHelp={showHelp} />
          </div>

          <ToolUsageTable tools={analysis.tool_consumptions} showHelp={showHelp} />
        </div>
      )}
    </div>
  )
}
