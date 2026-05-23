import { useState, useEffect, useCallback, useRef } from 'react'
import { Bot, MonitorPlay, Monitor, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useCCSessions } from './useCCSessions'
import { SessionList } from './SessionList'
import { TerminalView } from './TerminalView'
import type { TerminalViewHandle } from './TerminalView'
import { AISuggestPanel } from './AISuggestPanel'
import { NewSessionDialog } from './NewSessionDialog'
import { KillSessionDialog } from './KillSessionDialog'
import type { CCSession } from './types'

const MAX_GRID_PANES = 4

function addTarget(prev: string[], target: string): string[] {
  if (prev.includes(target)) return prev
  if (prev.length >= MAX_GRID_PANES) return prev
  return [...prev, target]
}

export function CCBridgePage() {
  const { sessions, loading, error, refresh } = useCCSessions()
  const [activeTargets, setActiveTargets] = useState<string[]>([])
  const [fullscreenTarget, setFullscreenTarget] = useState<string | null>(null)
  const [focusedTarget, setFocusedTarget] = useState<string | null>(null)
  const [newSessionOpen, setNewSessionOpen] = useState(false)
  const [killSession, setKillSession] = useState<CCSession | null>(null)
  const [sidebarOpen, setSidebarOpen] = useState(true)
  /** Whether the AI panel is visible (desktop: right column; mobile: drawer). */
  const [aiPanelOpen, setAiPanelOpen] = useState(false)

  const isFullscreen = fullscreenTarget !== null

  // Map from target → TerminalView ref (so AISuggestPanel can reach any pane's handle)
  const terminalRefs = useRef<Map<string, TerminalViewHandle>>(new Map())

  // ESC exits fullscreen
  useEffect(() => {
    if (!isFullscreen) return
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setFullscreenTarget(null)
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [isFullscreen])

  const toggleTarget = useCallback((target: string) => {
    setActiveTargets((prev) =>
      prev.includes(target)
        ? prev.filter((t) => t !== target)
        : addTarget(prev, target),
    )
    setFullscreenTarget((cur) => (cur === target ? null : cur))
  }, [])

  const removeTarget = useCallback((target: string) => {
    setActiveTargets((prev) => prev.filter((t) => t !== target))
    setFullscreenTarget((cur) => (cur === target ? null : cur))
    terminalRefs.current.delete(target)
  }, [])

  const handleSpawned = (tmuxTarget: string) => {
    refresh()
    setActiveTargets((prev) => addTarget(prev, tmuxTarget))
  }

  const handleKilled = () => {
    if (killSession) {
      removeTarget(killSession.tmux_target)
    }
    setKillSession(null)
    refresh()
  }

  // ---------------------------------------------------------------------------
  // AI panel callbacks — routed to the focused (or last) terminal pane.
  // ---------------------------------------------------------------------------

  const getActiveHandle = useCallback((): TerminalViewHandle | null => {
    // Prefer the focused pane; fall back to the last active target.
    const targets = activeTargets
    const preferred = focusedTarget ?? targets[targets.length - 1] ?? null
    return preferred ? (terminalRefs.current.get(preferred) ?? null) : null
  }, [activeTargets, focusedTarget])

  const writeStdin = useCallback(
    (s: string) => {
      getActiveHandle()?.writeStdin(s)
    },
    [getActiveHandle],
  )

  const getTerminalContext = useCallback((): string => {
    return getActiveHandle()?.getContext() ?? ''
  }, [getActiveHandle])

  // ---------------------------------------------------------------------------

  const gridCols =
    activeTargets.length <= 1
      ? 'grid-cols-1'
      : activeTargets.length === 2
        ? 'grid-cols-1 md:grid-cols-2'
        : 'grid-cols-1 md:grid-cols-2 lg:grid-cols-2'

  return (
    <div
      className={cn(
        'flex flex-col',
        isFullscreen
          ? 'fixed inset-0 z-50 bg-background'
          : 'h-[calc(100vh-10rem)] md:h-[calc(100vh-8.5rem)] border rounded-lg overflow-hidden',
      )}
    >
      {!isFullscreen && (
        <div className="flex items-center gap-3 px-3 py-2 md:px-4 md:py-3 border-b shrink-0 bg-muted/30">
          <MonitorPlay className="h-4 w-4 md:h-5 md:w-5 shrink-0" />
          <div className="flex items-baseline gap-2 flex-wrap min-w-0 overflow-hidden">
            <h1 className="text-sm md:text-base font-semibold truncate">
              CC Bridge
            </h1>
            <span className="text-[10px] md:text-xs text-muted-foreground truncate sm:whitespace-normal">
              Observe Claude Code sessions in tmux.
            </span>
          </div>
          <div className="ml-auto flex items-center gap-1">
            {/* AI panel toggle — always visible */}
            <Button
              variant={aiPanelOpen ? 'secondary' : 'ghost'}
              size="sm"
              className="h-8 px-2 gap-1.5 text-xs"
              onClick={() => setAiPanelOpen(!aiPanelOpen)}
              title="Toggle AI suggestion panel"
              data-testid="ai-panel-toggle"
            >
              <Bot className="h-4 w-4" />
              <span className="hidden sm:inline">AI</span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="ml-1 md:hidden h-8 w-8 p-0"
              onClick={() => setSidebarOpen(!sidebarOpen)}
            >
              <Monitor className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}

      {/* Main body — sessions sidebar + terminal grid + AI panel */}
      <div className="flex flex-1 min-h-0 flex-col md:flex-row">
        {/* Session sidebar */}
        {!isFullscreen && (
          <div
            className={cn(
              'border-r shrink-0 overflow-hidden transition-all duration-300',
              sidebarOpen
                ? 'w-full md:w-52 border-b md:border-b-0 h-[200px] md:h-auto'
                : 'w-0 h-0 border-0',
            )}
          >
            <SessionList
              sessions={sessions}
              loading={loading}
              error={error}
              activeTargets={activeTargets}
              onToggleTarget={toggleTarget}
              onRefresh={refresh}
              onNewSession={() => setNewSessionOpen(true)}
              onKillSession={setKillSession}
            />
          </div>
        )}

        {/* Terminal pane area */}
        <div className="flex-1 min-w-0 relative flex flex-col md:flex-row min-h-0">
          {/* Terminal grid */}
          <div
            className={cn(
              'flex-1 relative min-h-0 min-w-0',
              // On desktop, give room to the AI panel when open
              aiPanelOpen && 'md:flex-1',
            )}
          >
            {activeTargets.length === 0 ? (
              <div className="absolute inset-0 flex flex-col items-center justify-center text-muted-foreground bg-background p-4 text-center">
                <Monitor className="h-8 w-8 md:h-12 md:w-12 mb-3" />
                <p className="text-xs md:text-sm">Select a session to attach</p>
                <Button
                  variant="outline"
                  size="sm"
                  className="mt-4 md:hidden"
                  onClick={() => setSidebarOpen(true)}
                >
                  Open Session List
                </Button>
              </div>
            ) : (
              <div
                className={cn(
                  'absolute inset-0 grid auto-rows-fr overflow-y-auto md:overflow-hidden',
                  isFullscreen ? 'grid-cols-1' : gridCols,
                )}
              >
                {activeTargets.map((target) => {
                  const isThisFullscreen = fullscreenTarget === target
                  const hidden = isFullscreen && !isThisFullscreen
                  return (
                    <div
                      key={target}
                      className={cn(
                        hidden
                          ? 'hidden'
                          : 'relative min-h-0 min-w-0 overflow-hidden',
                        !isFullscreen &&
                          !hidden &&
                          'border-b border-r last:border-r-0',
                        !hidden &&
                          (focusedTarget === target
                            ? 'bg-primary/60'
                            : 'bg-border/30'),
                      )}
                      onMouseDown={() => setFocusedTarget(target)}
                      onFocusCapture={() => setFocusedTarget(target)}
                    >
                      <div
                        className={cn(
                          'absolute inset-[2px] rounded-sm',
                          hidden && 'inset-0 rounded-none',
                        )}
                      >
                        <TerminalView
                          ref={(handle) => {
                            if (handle) {
                              terminalRefs.current.set(target, handle)
                            } else {
                              terminalRefs.current.delete(target)
                            }
                          }}
                          target={target}
                          fullscreen={isThisFullscreen}
                          onToggleFullscreen={() =>
                            setFullscreenTarget(isThisFullscreen ? null : target)
                          }
                          onClose={() => removeTarget(target)}
                        />
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </div>

          {/* AI suggest panel — desktop: right column; mobile: bottom drawer */}
          {aiPanelOpen && (
            <>
              {/* Desktop panel (md+) */}
              <div
                className="hidden md:flex flex-col w-96 shrink-0 border-l"
                data-testid="ai-panel"
              >
                <AISuggestPanel
                  writeStdin={writeStdin}
                  getTerminalContext={getTerminalContext}
                />
              </div>

              {/* Mobile drawer (< md) */}
              <div
                className="md:hidden fixed inset-x-0 bottom-0 z-40 flex flex-col bg-background border-t shadow-lg"
                style={{ height: '55vh' }}
                data-testid="ai-panel-mobile"
              >
                <div className="flex items-center justify-between px-3 py-1.5 border-b bg-muted/30">
                  <div className="flex items-center gap-2">
                    <Bot className="h-4 w-4 text-primary" />
                    <span className="text-sm font-medium">AI Suggest</span>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6"
                    onClick={() => setAiPanelOpen(false)}
                    aria-label="Close AI panel"
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </div>
                <div className="flex-1 min-h-0">
                  <AISuggestPanel
                    writeStdin={writeStdin}
                    getTerminalContext={getTerminalContext}
                  />
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      <NewSessionDialog
        open={newSessionOpen}
        onOpenChange={setNewSessionOpen}
        onSpawned={handleSpawned}
      />

      <KillSessionDialog
        open={killSession !== null}
        onOpenChange={(open) => {
          if (!open) setKillSession(null)
        }}
        session={killSession}
        isWorktreeSession={false}
        onKilled={handleKilled}
      />
    </div>
  )
}
