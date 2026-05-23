import { useRef, useEffect, useImperativeHandle, forwardRef } from 'react'
import { Monitor, Maximize2, Minimize2, X } from 'lucide-react'
import { useTerminal } from './useTerminal'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface TerminalViewProps {
  target: string | null
  fullscreen?: boolean
  onToggleFullscreen?: () => void
  onClose?: () => void
}

/** Imperative handle exposed to the parent via forwardRef. */
export interface TerminalViewHandle {
  /** Write a string directly to the PTY stdin. */
  writeStdin: (s: string) => void
  /** Return the last `n` lines from the active xterm buffer (default 40). */
  getContext: (lines?: number) => string
}

export const TerminalView = forwardRef<TerminalViewHandle, TerminalViewProps>(
  function TerminalView(
    { target, fullscreen, onToggleFullscreen, onClose }: TerminalViewProps,
    ref,
  ) {
    const wrapperRef = useRef<HTMLDivElement>(null)
    const containerRef = useRef<HTMLDivElement>(null)
    const { connected, exitCode, readOnly, setReadOnly, attach, detach, termRef, wsRef } =
      useTerminal(containerRef, wrapperRef)

    useEffect(() => {
      if (target) {
        attach(target)
      } else {
        detach()
      }
    }, [target, attach, detach])

    // Expose imperative handle so CCBridgePage can pass writeStdin / getContext
    // to AISuggestPanel without lifting xterm state up.
    useImperativeHandle(
      ref,
      () => ({
        writeStdin(s: string) {
          const ws = wsRef.current
          if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(new TextEncoder().encode(s))
          }
        },
        getContext(lines = 40) {
          const term = termRef.current
          if (!term) return ''
          const buf = term.buffer.active
          const totalRows = buf.length
          const start = Math.max(0, totalRows - lines)
          const result: string[] = []
          for (let i = start; i < totalRows; i++) {
            const line = buf.getLine(i)
            if (line) {
              result.push(line.translateToString(true))
            }
          }
          // Trim trailing empty lines
          while (result.length > 0 && result[result.length - 1].trim() === '') {
            result.pop()
          }
          return result.join('\n')
        },
      }),
      [termRef, wsRef],
    )

    return (
      <div className="flex flex-col h-full">
        <div ref={wrapperRef} className="flex-1 relative overflow-hidden">
          {!target && (
            <div className="absolute inset-0 flex flex-col items-center justify-center text-muted-foreground bg-background">
              <Monitor className="h-12 w-12 mb-3" />
              <p className="text-sm">Select a session to attach</p>
            </div>
          )}
          <div
            ref={containerRef}
            data-testid="terminal-ready"
            className={cn('absolute inset-0', !target && 'invisible')}
          />
        </div>

        {target && (
          <div className="flex items-center justify-between px-3 py-2 border-t bg-background shrink-0">
            <div className="flex items-center gap-3">
              {/* Read-only / Interactive toggle */}
              <div className="flex items-center gap-1 text-sm">
                <button
                  className={cn(
                    'px-2 py-0.5 rounded text-xs font-medium transition-colors',
                    readOnly
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                  onClick={() => setReadOnly(true)}
                >
                  Read-only
                </button>
                <button
                  className={cn(
                    'px-2 py-0.5 rounded text-xs font-medium transition-colors',
                    !readOnly
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                  onClick={() => setReadOnly(false)}
                >
                  Interactive
                </button>
              </div>

              {/* Connection / exit state */}
              <span
                className={cn(
                  'text-xs',
                  exitCode !== null
                    ? 'text-muted-foreground'
                    : connected
                      ? 'text-green-500'
                      : 'text-muted-foreground',
                )}
              >
                {exitCode !== null
                  ? `Exited (${exitCode})`
                  : connected
                    ? 'Connected'
                    : 'Disconnected'}
              </span>
            </div>

            <div className="flex items-center gap-2">
              {onToggleFullscreen && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={onToggleFullscreen}
                  title={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                >
                  {fullscreen ? (
                    <Minimize2 className="h-3.5 w-3.5" />
                  ) : (
                    <Maximize2 className="h-3.5 w-3.5" />
                  )}
                </Button>
              )}
              {onClose && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={onClose}
                  title="Close pane"
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              )}
              {connected ? (
                <Button variant="outline" size="sm" onClick={detach}>
                  Detach
                </Button>
              ) : (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => attach(target)}
                  disabled={exitCode !== null}
                >
                  {exitCode !== null ? 'Exited' : 'Attach'}
                </Button>
              )}
            </div>
          </div>
        )}
      </div>
    )
  },
)
