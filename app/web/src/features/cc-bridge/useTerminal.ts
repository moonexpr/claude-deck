/**
 * useTerminal — xterm.js hook for the portable-pty wire contract.
 *
 * Wire contract:
 *   - After WS upgrade, send an Open frame (text JSON) as the first message.
 *   - Keystrokes are sent as binary (UTF-8 encoded Uint8Array).
 *   - Server stdout comes as binary frames → written directly to xterm.
 *   - Resize is sent as text JSON {"kind":"resize","cols":N,"rows":N}.
 *   - Server sends {"kind":"exit","code":N} when the child exits (text JSON).
 *
 * The legacy `mode` param (readonly / interactive) is replaced by a simple
 * `readOnly` toggle on the hook state.  When readOnly is true, keystrokes are
 * suppressed client-side — no protocol difference needed on the server.
 */
import { useRef, useCallback, useState, useEffect } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import '@xterm/xterm/css/xterm.css'
import { fetchTerminalToken, buildTerminalWsUrl } from './api'
import type { OpenFrame, ResizeFrame, ExitFrame } from './types'

const DEFAULT_CMD = ['claude']
const DEFAULT_CWD = '/'

export function useTerminal(
  containerRef: React.RefObject<HTMLDivElement | null>,
  wrapperRef: React.RefObject<HTMLDivElement | null>,
) {
  const termRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const [connected, setConnected] = useState(false)
  const [exitCode, setExitCode] = useState<number | null>(null)
  const [readOnly, setReadOnly] = useState(true)
  // Keep a ref in sync so event handlers (closures) always see the latest value.
  const readOnlyRef = useRef(true)

  useEffect(() => {
    readOnlyRef.current = readOnly
  }, [readOnly])

  // ---------------------------------------------------------------------------
  // Terminal initialisation (idempotent — safe to call twice in StrictMode)
  // ---------------------------------------------------------------------------

  const initTerminal = useCallback(() => {
    if (termRef.current || !containerRef.current) return

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
      theme: {
        background: '#1e1e2e',
        foreground: '#cdd6f4',
        cursor: '#f5e0dc',
      },
    })

    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)
    term.loadAddon(new WebLinksAddon())
    term.open(containerRef.current)
    fitAddon.fit()

    termRef.current = term
    fitAddonRef.current = fitAddon
  }, [containerRef])

  // ---------------------------------------------------------------------------
  // Attach — connect a WebSocket for the given target and send the Open frame
  // ---------------------------------------------------------------------------

  const attach = useCallback(
    async (
      target: string,
      opts?: { cmd?: string[]; cwd?: string; env?: Record<string, string> },
    ) => {
      // Close any existing connection first.
      if (wsRef.current) {
        wsRef.current.close()
        wsRef.current = null
      }

      initTerminal()
      const term = termRef.current
      if (!term) return

      term.clear()
      setExitCode(null)

      const { token } = await fetchTerminalToken()

      // Guard against StrictMode double-mount race: if the terminal was
      // disposed and replaced during the async token fetch, bail out.
      if (termRef.current !== term) return

      const url = buildTerminalWsUrl(target, token)
      const ws = new WebSocket(url)
      ws.binaryType = 'arraybuffer'
      wsRef.current = ws

      ws.onopen = () => {
        setConnected(true)

        // Fit before sending Open so col/row counts are accurate.
        requestAnimationFrame(() => {
          fitAddonRef.current?.fit()
          const dims = fitAddonRef.current?.proposeDimensions() ?? {
            cols: 80,
            rows: 24,
          }

          const openFrame: OpenFrame = {
            kind: 'open',
            cmd: opts?.cmd ?? DEFAULT_CMD,
            cwd: opts?.cwd ?? DEFAULT_CWD,
            env: opts?.env ?? {},
            cols: dims.cols,
            rows: dims.rows,
          }
          ws.send(JSON.stringify(openFrame))
        })
      }

      ws.onmessage = (event) => {
        if (event.data instanceof ArrayBuffer) {
          // Binary frame = stdout/stderr from pty
          term.write(new Uint8Array(event.data))
        } else if (typeof event.data === 'string') {
          try {
            const msg = JSON.parse(event.data) as { kind: string } & Partial<ExitFrame>
            if (msg.kind === 'exit') {
              setExitCode(msg.code ?? -1)
              term.writeln(`\r\n\x1b[90m[process exited with code ${msg.code ?? -1}]\x1b[0m`)
            } else if (msg.kind === 'error') {
              const err = (msg as unknown as { message?: string }).message ?? 'unknown error'
              term.writeln(`\r\n\x1b[31mError: ${err}\x1b[0m`)
            }
          } catch {
            // Unexpected text — write it verbatim.
            term.write(event.data)
          }
        }
      }

      ws.onclose = () => {
        setConnected(false)
      }

      ws.onerror = () => {
        setConnected(false)
      }

      // Keystrokes → binary frames (suppressed in read-only mode)
      const onDataDisposable = term.onData((data) => {
        if (!readOnlyRef.current && ws.readyState === WebSocket.OPEN) {
          const encoded = new TextEncoder().encode(data)
          ws.send(encoded)
        }
      })

      // Resize → text JSON frame
      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        if (ws.readyState === WebSocket.OPEN) {
          const frame: ResizeFrame = { kind: 'resize', cols, rows }
          ws.send(JSON.stringify(frame))
        }
      })

      ws.addEventListener(
        'close',
        () => {
          onDataDisposable.dispose()
          onResizeDisposable.dispose()
        },
        { once: true },
      )
    },
    [initTerminal],
  )

  const detach = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close()
      wsRef.current = null
    }
    setConnected(false)
    termRef.current?.clear()
    termRef.current?.writeln('\x1b[90mDetached.\x1b[0m')
  }, [])

  // ---------------------------------------------------------------------------
  // ResizeObserver — keep xterm sized to its wrapper element
  // ---------------------------------------------------------------------------

  useEffect(() => {
    const wrapper = wrapperRef.current
    if (!wrapper) return

    let rafId: number | null = null
    const observer = new ResizeObserver(() => {
      if (rafId) cancelAnimationFrame(rafId)
      rafId = requestAnimationFrame(() => {
        if (!fitAddonRef.current) return
        fitAddonRef.current.fit()
        const dims = fitAddonRef.current.proposeDimensions()
        if (dims && wsRef.current?.readyState === WebSocket.OPEN) {
          const frame: ResizeFrame = { kind: 'resize', cols: dims.cols, rows: dims.rows }
          wsRef.current.send(JSON.stringify(frame))
        }
      })
    })
    observer.observe(wrapper)
    return () => {
      observer.disconnect()
      if (rafId) cancelAnimationFrame(rafId)
    }
  }, [wrapperRef])

  // ---------------------------------------------------------------------------
  // Cleanup on unmount
  // ---------------------------------------------------------------------------

  useEffect(() => {
    return () => {
      wsRef.current?.close()
      wsRef.current = null
      termRef.current?.dispose()
      termRef.current = null
      fitAddonRef.current = null
    }
  }, [])

  return { connected, exitCode, readOnly, setReadOnly, attach, detach }
}
