// Types for the cc-bridge feature.

export interface CCSession {
  tmux_target: string
  session_name: string
  window_name: string
  pane_id: string
  cwd: string
  pid: string
  status: string
}

export interface CCSessionsResponse {
  sessions: CCSession[]
  count: number
}

export interface CCPreviewResponse {
  target: string
  content: string
}

export interface CCTokenResponse {
  token: string
}

export interface SpawnSessionRequest {
  directory: string
  mode: 'plain' | 'worktree' | 'resume'
  worktree_name?: string
  session_id?: string
  project_folder?: string
  skip_permissions?: boolean
}

export interface SpawnSessionResponse {
  tmux_target: string
  session_name: string
}

export interface KillSessionResponse {
  killed: boolean
  error?: string
}

// ---------------------------------------------------------------------------
// Wire-protocol frame types (used by useTerminal)
// ---------------------------------------------------------------------------

/** Sent as the first text frame after WS upgrade (client→server). */
export interface OpenFrame {
  kind: 'open'
  cmd: string[]
  cwd: string
  env: Record<string, string>
  cols: number
  rows: number
}

/** Resize request — either direction. */
export interface ResizeFrame {
  kind: 'resize'
  cols: number
  rows: number
}

/** Signal request (client→server). */
export interface SignalFrame {
  kind: 'signal'
  sig: 'INT' | 'TERM' | 'KILL'
}

/** Sent by the server when the child exits (terminal frame). */
export interface ExitFrame {
  kind: 'exit'
  code: number
}
