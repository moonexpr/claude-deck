import { useState, useEffect } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { useTranscript } from '../TranscriptContext'

interface Props {
  tool_use_id: string
  content: string | Record<string, unknown> | unknown[]
  is_error: boolean
}

/** Flatten tool_result content (string | object | array of text blocks) to text. */
function contentToString(content: Props['content']): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((item) => {
        if (typeof item === 'string') return item
        if (item && typeof item === 'object' && 'text' in item) {
          return String((item as { text: unknown }).text ?? '')
        }
        return JSON.stringify(item)
      })
      .join('\n')
  }
  if (content && typeof content === 'object' && 'text' in content) {
    return String((content as { text: unknown }).text ?? '')
  }
  return JSON.stringify(content, null, 2)
}

const PREVIEW_LINES = 20

export function ToolResultBlock({ tool_use_id, content, is_error }: Props) {
  const { expandSignal } = useTranscript()
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (expandSignal.nonce > 0) setExpanded(expandSignal.expand)
  }, [expandSignal.nonce, expandSignal.expand])

  const full = contentToString(content)
  const lines = full.split('\n')
  const isLong = lines.length > PREVIEW_LINES
  const hiddenCount = lines.length - PREVIEW_LINES
  const shown = expanded || !isLong ? full : lines.slice(0, PREVIEW_LINES).join('\n')

  return (
    <div
      className={`border rounded-lg p-3 ${
        is_error ? 'border-red-500/50 bg-red-50/10' : 'border-green-500/50 bg-green-50/10'
      }`}
    >
      <div className="flex items-center gap-2 mb-2">
        <Badge variant={is_error ? 'destructive' : 'outline'}>
          {is_error ? 'Error' : 'Result'}
        </Badge>
        <span className="text-xs text-muted-foreground truncate">{tool_use_id}</span>
      </div>

      {full.trim() === '' ? (
        <p className="text-xs text-muted-foreground italic">(empty result)</p>
      ) : (
        <pre className="text-xs whitespace-pre-wrap overflow-x-auto">{shown}</pre>
      )}

      {isLong && (
        <Button variant="ghost" size="sm" onClick={() => setExpanded(!expanded)} className="mt-2">
          {expanded ? (
            <>
              <ChevronUp className="h-4 w-4 mr-1" />
              Show less
            </>
          ) : (
            <>
              <ChevronDown className="h-4 w-4 mr-1" />
              Show {hiddenCount.toLocaleString()} more lines
            </>
          )}
        </Button>
      )}
    </div>
  )
}
