import { useState, useEffect } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ChevronDown, ChevronUp, CheckSquare, Square, Clock } from 'lucide-react'
import { CodeHighlight } from '@/components/shared/CodeHighlight'
import { DiffView } from './DiffView'
import { toolStyle, languageFromPath } from '../toolMeta'
import { useTranscript } from '../TranscriptContext'

interface Props {
  name: string
  id: string
  input: Record<string, unknown>
}

// Static class strings (Tailwind JIT-safe) keyed by hue token.
const HUE: Record<string, { border: string; text: string }> = {
  purple: { border: 'border-purple-500/50 bg-purple-50/10', text: 'text-purple-700' },
  sky: { border: 'border-sky-500/50 bg-sky-50/10', text: 'text-sky-700' },
  blue: { border: 'border-blue-500/50 bg-blue-50/10', text: 'text-blue-700' },
  teal: { border: 'border-teal-500/50 bg-teal-50/10', text: 'text-teal-700' },
  amber: { border: 'border-amber-500/50 bg-amber-50/10', text: 'text-amber-700' },
  fuchsia: { border: 'border-fuchsia-500/50 bg-fuchsia-50/10', text: 'text-fuchsia-700' },
  cyan: { border: 'border-cyan-500/50 bg-cyan-50/10', text: 'text-cyan-700' },
  gray: { border: 'border-gray-500/50 bg-gray-50/10', text: 'text-gray-700' },
}

const str = (v: unknown): string => (typeof v === 'string' ? v : '')

interface TodoItem {
  content?: string
  status?: string
  activeForm?: string
}

export function ToolUseBlock({ name, input }: Props) {
  const { icon: Icon, hue } = toolStyle(name)
  const hueCls = HUE[hue] ?? HUE.gray
  const sub = subtitle(name, input)

  return (
    <div className={`border rounded-lg p-3 ${hueCls.border}`}>
      <div className="flex items-center gap-2 mb-2">
        <Icon className={`h-4 w-4 ${hueCls.text}`} />
        <Badge variant="outline" className={hueCls.text}>{name}</Badge>
        {sub && (
          <span className="text-xs text-muted-foreground truncate" title={sub}>
            {sub}
          </span>
        )}
      </div>
      <ToolBody name={name} input={input} />
    </div>
  )
}

/** Header subtitle (file path, pattern, url, …) per tool. */
function subtitle(name: string, input: Record<string, unknown>): string {
  switch (name) {
    case 'Read':
    case 'Write':
    case 'Edit':
    case 'MultiEdit':
    case 'NotebookEdit':
      return str(input.file_path) || str(input.notebook_path)
    case 'Grep':
      return str(input.pattern)
    case 'Glob':
      return str(input.pattern)
    case 'LS':
      return str(input.path)
    case 'WebFetch':
      return str(input.url)
    case 'WebSearch':
      return str(input.query)
    case 'Task':
    case 'Agent':
      return str(input.subagent_type) || str(input.description)
    default:
      return ''
  }
}

function ToolBody({ name, input }: { name: string; input: Record<string, unknown> }) {
  switch (name) {
    case 'Bash':
      return (
        <>
          <CodeHighlight code={str(input.command)} language="bash" />
          {str(input.description) && (
            <p className="text-xs text-muted-foreground mt-1 italic">{str(input.description)}</p>
          )}
        </>
      )

    case 'Write':
      return <CollapsibleCode code={str(input.content)} language={languageFromPath(str(input.file_path))} />

    case 'Edit':
      return <DiffView oldText={str(input.old_string)} newText={str(input.new_string)} />

    case 'MultiEdit': {
      const edits = Array.isArray(input.edits) ? (input.edits as Record<string, unknown>[]) : []
      return (
        <div className="space-y-2">
          {edits.map((e, i) => (
            <DiffView key={i} oldText={str(e.old_string)} newText={str(e.new_string)} />
          ))}
        </div>
      )
    }

    case 'TodoWrite': {
      const todos = Array.isArray(input.todos) ? (input.todos as TodoItem[]) : []
      return (
        <ul className="space-y-1 text-sm">
          {todos.map((t, i) => {
            const done = t.status === 'completed'
            const active = t.status === 'in_progress'
            return (
              <li key={i} className="flex items-start gap-2">
                {done ? (
                  <CheckSquare className="h-4 w-4 mt-0.5 text-green-600 shrink-0" />
                ) : active ? (
                  <Clock className="h-4 w-4 mt-0.5 text-amber-600 shrink-0" />
                ) : (
                  <Square className="h-4 w-4 mt-0.5 text-muted-foreground shrink-0" />
                )}
                <span className={done ? 'line-through text-muted-foreground' : active ? 'font-medium' : ''}>
                  {active ? (t.activeForm || t.content) : t.content}
                </span>
              </li>
            )
          })}
        </ul>
      )
    }

    case 'Task':
    case 'Agent':
      return (
        <div className="text-sm">
          <p className="text-muted-foreground whitespace-pre-wrap">{str(input.prompt) || str(input.description)}</p>
        </div>
      )

    case 'Grep':
      return (
        <div className="text-xs text-muted-foreground space-y-0.5">
          {str(input.path) && <div>path: <code>{str(input.path)}</code></div>}
          {str(input.glob) && <div>glob: <code>{str(input.glob)}</code></div>}
          {str(input.output_mode) && <div>mode: {str(input.output_mode)}</div>}
        </div>
      )

    case 'WebFetch':
      return str(input.prompt) ? (
        <p className="text-xs text-muted-foreground">{str(input.prompt)}</p>
      ) : null

    case 'Read':
    case 'Glob':
    case 'LS':
    case 'WebSearch':
    case 'NotebookEdit':
      // Header subtitle already carries the salient field.
      return null

    default:
      return (
        <pre className="text-xs overflow-x-auto">{JSON.stringify(input, null, 2)}</pre>
      )
  }
}

const CONTENT_CAP = 2000

/** Code preview that caps long content and honors Expand/Collapse-all. */
function CollapsibleCode({ code, language }: { code: string; language: string }) {
  const { expandSignal } = useTranscript()
  const isLong = code.length > CONTENT_CAP
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (expandSignal.nonce > 0) setExpanded(expandSignal.expand)
  }, [expandSignal.nonce, expandSignal.expand])

  if (!code) return null
  const shown = expanded || !isLong ? code : code.slice(0, CONTENT_CAP)

  return (
    <>
      <CodeHighlight code={shown} language={language} />
      {isLong && (
        <Button variant="ghost" size="sm" onClick={() => setExpanded(!expanded)} className="mt-1">
          {expanded ? <ChevronUp className="h-4 w-4 mr-1" /> : <ChevronDown className="h-4 w-4 mr-1" />}
          {expanded ? 'Show less' : `Show full (${code.length.toLocaleString()} chars)`}
        </Button>
      )}
    </>
  )
}
