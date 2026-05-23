import { useState, useEffect, useCallback } from 'react'
import { useParams, useSearchParams, useNavigate } from 'react-router-dom'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Progress } from '@/components/ui/progress'
import { ChevronLeft, ChevronRight, ArrowLeft } from 'lucide-react'
import { apiClient } from '@/lib/api'
import { useSessionsApi } from './useSessionsApi'
import { ConversationList } from './ConversationList'
import type { SessionDetail } from '@/types/sessions'
import type { ContextAnalysis, ContextAnalysisResponse } from '@/types/context'

export function SessionViewPage() {
  const { projectFolder, sessionId } = useParams<{ projectFolder: string; sessionId: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const navigate = useNavigate()
  const { getSessionDetail } = useSessionsApi()

  const [session, setSession] = useState<SessionDetail | null>(null)
  const [currentPage, setCurrentPage] = useState(1)
  const [totalPages, setTotalPages] = useState(1)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [contextAnalysis, setContextAnalysis] = useState<ContextAnalysis | null>(null)
  const [contextLoading, setContextLoading] = useState(false)
  const [contextLoaded, setContextLoaded] = useState(false)

  const loadSession = useCallback(async (page: number) => {
    if (!projectFolder || !sessionId) return

    setLoading(true)
    setError(null)
    try {
      const data = await getSessionDetail(projectFolder, sessionId, page)
      setSession(data.session)
      setCurrentPage(data.current_page)
      setTotalPages(data.total_pages)
    } catch (err) {
      console.error('Failed to load session:', err)
      setError(err instanceof Error ? err.message : 'Failed to load session')
    } finally {
      setLoading(false)
    }
  }, [getSessionDetail, projectFolder, sessionId])

  const loadContext = useCallback(async () => {
    if (!projectFolder || !sessionId || contextLoaded) return

    setContextLoading(true)
    try {
      const data = await apiClient<ContextAnalysisResponse>(
        `context/${projectFolder}/${sessionId}`
      )
      setContextAnalysis(data.analysis)
    } catch {
      // Context analysis may fail for some sessions
    } finally {
      setContextLoading(false)
      setContextLoaded(true)
    }
  }, [projectFolder, sessionId, contextLoaded])

  useEffect(() => {
    const page = parseInt(searchParams.get('page') ?? '1', 10)
    setCurrentPage(page)
    loadSession(page)
  }, [sessionId, projectFolder, searchParams, loadSession])

  const handlePageChange = (newPage: number) => {
    setSearchParams({ page: newPage.toString() })
  }

  const handleBack = () => {
    navigate('/sessions')
  }

  const handleTabChange = (value: string) => {
    if (value === 'context' && !contextLoaded) {
      loadContext()
    }
  }

  if (!projectFolder || !sessionId) {
    return (
      <div className="space-y-6">
        <div className="text-center py-8">
          <p className="text-muted-foreground">Invalid session URL</p>
          <Button onClick={handleBack} className="mt-4">
            Back to Sessions
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button variant="ghost" size="sm" onClick={handleBack}>
          <ArrowLeft className="h-4 w-4 mr-2" />
          Back
        </Button>
        <div className="flex-1">
          <div className="flex items-center gap-2 mb-1">
            <h1 className="text-3xl font-bold">Session: {sessionId}</h1>
            {session && <Badge variant="secondary">{session.project_name}</Badge>}
          </div>
          <p className="text-muted-foreground">
            Full conversation view
          </p>
        </div>
      </div>

      {/* Error State */}
      {error && (
        <Card className="border-destructive">
          <CardHeader>
            <CardTitle className="text-destructive">Error</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm">{error}</p>
            <Button onClick={() => loadSession(currentPage)} className="mt-4">
              Retry
            </Button>
          </CardContent>
        </Card>
      )}

      {/* Loading State */}
      {loading && !session && (
        <div className="py-12 text-center">
          <p className="text-muted-foreground">Loading session...</p>
        </div>
      )}

      {/* Session Content */}
      {!loading && session && (
        <>
          {/* Session Stats */}
          <Card>
            <CardContent className="pt-6">
              <div className="flex gap-6 text-sm">
                <div className="flex flex-col">
                  <span className="text-muted-foreground">Messages</span>
                  <span className="text-2xl font-bold">{session.total_messages}</span>
                </div>
                <div className="flex flex-col">
                  <span className="text-muted-foreground">Tool Calls</span>
                  <span className="text-2xl font-bold">{session.total_tool_calls}</span>
                </div>
                {session.models_used.length > 0 && (
                  <div className="flex flex-col">
                    <span className="text-muted-foreground">Models</span>
                    <div className="flex gap-1 mt-1">
                      {session.models_used.map(model => (
                        <Badge key={model} variant="outline" className="text-xs">
                          {model}
                        </Badge>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </CardContent>
          </Card>

          {/* Tabbed content: Conversation / Context */}
          <Tabs defaultValue="conversation" onValueChange={handleTabChange}>
            <TabsList>
              <TabsTrigger value="conversation">Conversation</TabsTrigger>
              <TabsTrigger value="context">Context</TabsTrigger>
            </TabsList>

            <TabsContent value="conversation" className="space-y-4">
              <ConversationList conversations={session.conversations} />

              {/* Pagination */}
              {totalPages > 1 && (
                <Card>
                  <CardContent className="pt-6">
                    <div className="flex items-center justify-between">
                      <Button
                        variant="outline"
                        onClick={() => handlePageChange(currentPage - 1)}
                        disabled={currentPage === 1 || loading}
                      >
                        <ChevronLeft className="h-4 w-4 mr-1" />
                        Previous
                      </Button>
                      <span className="text-sm text-muted-foreground">
                        Page {currentPage} of {totalPages}
                      </span>
                      <Button
                        variant="outline"
                        onClick={() => handlePageChange(currentPage + 1)}
                        disabled={currentPage === totalPages || loading}
                      >
                        Next
                        <ChevronRight className="h-4 w-4 ml-1" />
                      </Button>
                    </div>
                  </CardContent>
                </Card>
              )}
            </TabsContent>

            <TabsContent value="context" className="space-y-4">
              {contextLoading && (
                <div className="py-8 text-center">
                  <p className="text-muted-foreground">Analyzing context...</p>
                </div>
              )}

              {!contextLoading && !contextAnalysis && contextLoaded && (
                <Card>
                  <CardContent className="py-8 text-center">
                    <p className="text-muted-foreground">No context data available for this session.</p>
                  </CardContent>
                </Card>
              )}

              {contextAnalysis && !contextLoading && (
                <ContextAnalysisPanel analysis={contextAnalysis} />
              )}
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Inline context analysis panel — avoids cross-feature imports.
// Displays the same data as the context feature's components without
// depending on them.
// ---------------------------------------------------------------------------

interface ContextAnalysisPanelProps {
  analysis: ContextAnalysis
}

function ContextAnalysisPanel({ analysis }: ContextAnalysisPanelProps) {
  const pct = Math.round(analysis.context_percentage)
  const zoneColor =
    pct >= 90 ? 'text-destructive' :
    pct >= 75 ? 'text-orange-500' :
    pct >= 50 ? 'text-yellow-500' :
    'text-green-500'

  return (
    <div className="space-y-4">
      {/* Overview row */}
      <div className="grid gap-4 md:grid-cols-3">
        {/* Context usage gauge */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Context Usage</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <div className={`text-4xl font-bold ${zoneColor}`}>{pct}%</div>
            <Progress value={pct} className="h-2" />
            <p className="text-xs text-muted-foreground">
              {analysis.current_context_tokens.toLocaleString()} / {analysis.max_context_tokens.toLocaleString()} tokens
            </p>
            <p className="text-xs text-muted-foreground capitalize">
              Zone: <span className={zoneColor}>{analysis.context_zone}</span>
            </p>
          </CardContent>
        </Card>

        {/* Projections */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Projections</CardTitle>
          </CardHeader>
          <CardContent className="space-y-1 text-sm">
            <div className="flex justify-between">
              <span className="text-muted-foreground">Avg tokens/turn</span>
              <span className="font-medium">{analysis.avg_tokens_per_turn.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Est. turns left</span>
              <span className="font-medium">{analysis.estimated_turns_remaining}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Total turns</span>
              <span className="font-medium">{analysis.total_turns}</span>
            </div>
          </CardContent>
        </Card>

        {/* Cache efficiency */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Cache Efficiency</CardTitle>
          </CardHeader>
          <CardContent className="space-y-1 text-sm">
            <div className="flex justify-between">
              <span className="text-muted-foreground">Hit ratio</span>
              <span className="font-medium">
                {(analysis.cache_efficiency.hit_ratio * 100).toFixed(1)}%
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Cache reads</span>
              <span className="font-medium">{analysis.cache_efficiency.total_cache_read.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Cache writes</span>
              <span className="font-medium">{analysis.cache_efficiency.total_cache_creation.toLocaleString()}</span>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* File consumptions */}
      {analysis.file_consumptions.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">File Consumption</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-1 text-xs">
              <div className="grid grid-cols-4 gap-2 font-medium text-muted-foreground pb-1 border-b">
                <span className="col-span-2">File</span>
                <span className="text-right">Reads</span>
                <span className="text-right">Tokens</span>
              </div>
              {analysis.file_consumptions.slice(0, 10).map((f, i) => (
                <div key={i} className="grid grid-cols-4 gap-2">
                  <span className="col-span-2 truncate text-muted-foreground" title={f.file_path}>
                    {f.file_path.split('/').pop() ?? f.file_path}
                  </span>
                  <span className="text-right">{f.read_count}</span>
                  <span className="text-right">{f.estimated_tokens.toLocaleString()}</span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Tool consumptions */}
      {analysis.tool_consumptions.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Tool Usage</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-1 text-xs">
              <div className="grid grid-cols-4 gap-2 font-medium text-muted-foreground pb-1 border-b">
                <span className="col-span-2">Tool</span>
                <span className="text-right">Calls</span>
                <span className="text-right">Tokens</span>
              </div>
              {analysis.tool_consumptions.slice(0, 10).map((t, i) => (
                <div key={i} className="grid grid-cols-4 gap-2">
                  <span className="col-span-2 truncate">{t.tool_name}</span>
                  <span className="text-right">{t.call_count}</span>
                  <span className="text-right">{t.total_result_tokens.toLocaleString()}</span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Snapshot timeline summary */}
      {analysis.snapshots.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Context Timeline</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-1">
              {analysis.snapshots.map((snap, i) => (
                <div key={i} className="flex items-center gap-2 text-xs">
                  <span className="w-12 text-muted-foreground text-right">T{snap.turn_number}</span>
                  <div className="flex-1">
                    <Progress value={snap.context_percentage} className="h-1.5" />
                  </div>
                  <span className="w-12 text-right text-muted-foreground">
                    {Math.round(snap.context_percentage)}%
                  </span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
