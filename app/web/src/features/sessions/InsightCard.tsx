import { useCallback, useEffect, useState } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { AlertTriangle, Sparkles, Check, X } from 'lucide-react'
import { apiClient } from '@/lib/api'
import { API_BASE_URL } from '@/lib/constants'

// Mirrors server_core::services::insight_service response shapes.
interface InsightArtifact {
  id: number
  kind: string // "Summary" | "Decision" | "Error" | "Follow-up"
  body: string
  severity: string
  source_locator: string | null
  quote: string | null
  status: string
}
interface JudgmentCallOut {
  id: number
  summary: string
  options: string[]
  chosen: string | null
  rationale: string | null
  source_locator: string | null
  quote: string | null
  status: string
}
interface SessionInsight {
  run_id: number
  status: string
  insights: InsightArtifact[]
  judgment_calls: JudgmentCallOut[]
  dropped: { citation_error: number; groundedness_error: number }
}

interface Props {
  projectFolder: string
  sessionId: string
}

/** Provenance line shown under every grounded item: the cited locator + the
 *  verbatim quote that grounds it (the evidence, inline). */
function Evidence({ locator, quote }: { locator: string | null; quote: string | null }) {
  if (!locator && !quote) return null
  return (
    <div className="mt-1.5 border-l-2 border-muted pl-2 text-xs text-muted-foreground">
      {quote && <span className="italic">“{quote}”</span>}
      {locator && (
        <code className="ml-2 font-mono text-[10px] opacity-70" title="source transcript entry">
          {locator}
        </code>
      )}
    </div>
  )
}

export function InsightCard({ projectFolder, sessionId }: Props) {
  const [insight, setInsight] = useState<SessionInsight | null>(null)
  const [loading, setLoading] = useState(true)
  const [analyzing, setAnalyzing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Non-null when the server reported no usable credential (503).
  const [unavailableKeySource, setUnavailableKeySource] = useState<
    'oauth' | 'api_key' | null | undefined
  >(undefined)

  const base = `insights/session/${projectFolder}/${sessionId}`

  // Load any previously-persisted insight on mount.
  useEffect(() => {
    let cancelled = false
    apiClient<SessionInsight>(base)
      .then((data) => {
        if (!cancelled) setInsight(data)
      })
      .catch(() => {
        /* 404 → never analyzed; leave insight null */
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [base])

  const analyze = useCallback(async () => {
    setAnalyzing(true)
    setError(null)
    setUnavailableKeySource(undefined)
    try {
      const res = await fetch(`${API_BASE_URL}${base}/analyze`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
      })
      const body = await res.json().catch(() => ({}))
      if (res.status === 503 && body?.status === 'unavailable') {
        setUnavailableKeySource(body.key_source ?? null)
        return
      }
      if (!res.ok) {
        setError(body?.detail || `Analysis failed (HTTP ${res.status})`)
        return
      }
      setInsight(body as SessionInsight)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Analysis failed')
    } finally {
      setAnalyzing(false)
    }
  }, [base])

  const setJudgmentStatus = useCallback(
    async (id: number, status: string) => {
      // optimistic
      setInsight((prev) =>
        prev
          ? {
              ...prev,
              judgment_calls: prev.judgment_calls.map((jc) =>
                jc.id === id ? { ...jc, status } : jc
              ),
            }
          : prev
      )
      try {
        await apiClient(`insights/judgment-call/${id}`, {
          method: 'PATCH',
          body: JSON.stringify({ status }),
        })
      } catch {
        /* best-effort; a reload reflects server truth */
      }
    },
    []
  )

  if (loading) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-muted-foreground">Loading insights…</CardContent>
      </Card>
    )
  }

  if (unavailableKeySource !== undefined) {
    return (
      <Card>
        <CardContent className="py-6">
          <div className="flex items-start gap-2 text-sm text-amber-800 dark:text-amber-300">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>
              {unavailableKeySource === 'oauth' ? (
                <>
                  Insight analysis needs a Claude Code OAuth bearer — run{' '}
                  <code className="font-mono text-xs">claude_ext</code> so it can be observed.
                </>
              ) : unavailableKeySource === 'api_key' ? (
                <>
                  Insight analysis unavailable — set{' '}
                  <code className="font-mono text-xs">ANTHROPIC_API_KEY</code> (keychain or env).
                </>
              ) : (
                <>
                  Insight analysis unavailable — configure an Anthropic credential (API key, or
                  OAuth via <code className="font-mono text-xs">claude_ext</code>).
                </>
              )}
            </span>
          </div>
        </CardContent>
      </Card>
    )
  }

  if (!insight) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-3 py-8 text-center">
          <p className="text-sm text-muted-foreground">
            Derive a structured insight from this session — decisions, judgment calls, and
            follow-ups, each grounded in the transcript.
          </p>
          <Button onClick={analyze} disabled={analyzing}>
            <Sparkles className="mr-2 h-4 w-4" />
            {analyzing ? 'Analyzing…' : 'Analyze session'}
          </Button>
          {error && <p className="text-sm text-destructive">{error}</p>}
        </CardContent>
      </Card>
    )
  }

  const byKind = (kind: string) => insight.insights.filter((i) => i.kind === kind)
  const summary = byKind('Summary')[0]
  const decisions = byKind('Decision')
  const errors = byKind('Error')
  const followUps = byKind('Follow-up')

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-muted-foreground">Session insight</h3>
        <Button variant="outline" size="sm" onClick={analyze} disabled={analyzing}>
          <Sparkles className="mr-1 h-3.5 w-3.5" />
          {analyzing ? 'Re-analyzing…' : 'Re-analyze'}
        </Button>
      </div>

      {summary && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Summary</CardTitle>
          </CardHeader>
          <CardContent className="text-sm">{summary.body}</CardContent>
        </Card>
      )}

      {insight.judgment_calls.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Judgment calls</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {insight.judgment_calls.map((jc) => (
              <div key={jc.id} className="text-sm">
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <span className={jc.status === 'dismissed' ? 'line-through opacity-60' : ''}>
                      {jc.summary}
                    </span>
                    {jc.chosen && (
                      <Badge variant="outline" className="ml-2 text-[10px]">
                        chose: {jc.chosen}
                      </Badge>
                    )}
                    {jc.status !== 'open' && (
                      <Badge variant="secondary" className="ml-2 text-[10px]">
                        {jc.status}
                      </Badge>
                    )}
                  </div>
                  <div className="flex shrink-0 gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      title="Accept"
                      onClick={() => setJudgmentStatus(jc.id, 'accepted')}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      title="Dismiss"
                      onClick={() => setJudgmentStatus(jc.id, 'dismissed')}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
                {jc.rationale && (
                  <p className="text-xs text-muted-foreground">{jc.rationale}</p>
                )}
                <Evidence locator={jc.source_locator} quote={jc.quote} />
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {decisions.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Decisions</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {decisions.map((d) => (
              <div key={d.id} className="text-sm">
                {d.body}
                <Evidence locator={d.source_locator} quote={d.quote} />
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {errors.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-destructive">Errors hit</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {errors.map((e) => (
              <div key={e.id} className="text-sm">
                {e.body}
                <Evidence locator={e.source_locator} quote={e.quote} />
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {followUps.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Follow-ups</CardTitle>
          </CardHeader>
          <CardContent>
            <ul className="list-disc space-y-1 pl-5 text-sm">
              {followUps.map((f) => (
                <li key={f.id}>{f.body}</li>
              ))}
            </ul>
          </CardContent>
        </Card>
      )}

      {(insight.dropped.citation_error > 0 || insight.dropped.groundedness_error > 0) && (
        <p className="text-xs text-muted-foreground">
          {insight.dropped.citation_error + insight.dropped.groundedness_error} ungrounded item(s)
          dropped by the provenance gate ({insight.dropped.citation_error} citation,{' '}
          {insight.dropped.groundedness_error} groundedness).
        </p>
      )}
    </div>
  )
}
