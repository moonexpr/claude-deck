import { apiClient, buildEndpoint } from '@/lib/api'
import type {
  CCSessionsResponse,
  CCPreviewResponse,
  CCTokenResponse,
  SpawnSessionRequest,
  SpawnSessionResponse,
  KillSessionResponse,
} from './types'
import type { SessionListResponse } from '@/types/sessions'

const BASE = 'cc-bridge'

export async function fetchCCSessions(): Promise<CCSessionsResponse> {
  return apiClient<CCSessionsResponse>(BASE + '/sessions')
}

export async function fetchSessionPreview(target: string): Promise<CCPreviewResponse> {
  return apiClient<CCPreviewResponse>(`${BASE}/sessions/${encodeURIComponent(target)}/preview`)
}

export async function fetchTerminalToken(): Promise<CCTokenResponse> {
  return apiClient<CCTokenResponse>(BASE + '/token')
}

/**
 * Build the WebSocket URL for the terminal endpoint.
 * Uses the same origin as the page so that the server-side same-origin check passes.
 */
export function buildTerminalWsUrl(target: string, token: string): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const host = window.location.host
  return `${protocol}//${host}/api/v1/${BASE}/sessions/${encodeURIComponent(target)}/terminal?token=${token}`
}

export async function spawnSession(request: SpawnSessionRequest): Promise<SpawnSessionResponse> {
  return apiClient<SpawnSessionResponse>(BASE + '/sessions', {
    method: 'POST',
    body: JSON.stringify(request),
  })
}

export async function killSession(
  target: string,
  cleanupWorktree: boolean = false,
): Promise<KillSessionResponse> {
  const params = cleanupWorktree ? '?cleanup_worktree=true' : ''
  return apiClient<KillSessionResponse>(
    `${BASE}/sessions/${encodeURIComponent(target)}${params}`,
    { method: 'DELETE' },
  )
}

/**
 * Fetch recent Claude Code session transcripts for the "Resume" mode picker
 * in NewSessionDialog.  This calls the /api/v1/sessions transcript endpoint
 * directly — no hook needed because the caller drives it from a useEffect.
 *
 * cc-bridge "sessions" (live PTY terminals) and transcript sessions are
 * different domains; this function lives here so cc-bridge imports nothing
 * from @/features/sessions/*.
 */
export async function listRecentSessions(params?: {
  limit?: number
  sort_by?: string
  sort_order?: string
}): Promise<SessionListResponse> {
  return apiClient<SessionListResponse>(buildEndpoint('sessions', params))
}
