import { useState, useEffect } from 'react'
import { ChevronDown, ChevronRight, Brain } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { MarkdownRenderer } from '@/components/shared/MarkdownRenderer'
import { useTranscript } from '../TranscriptContext'

interface Props {
  thinking: string
}

export function ThinkingBlock({ thinking }: Props) {
  const { showThinking, expandSignal } = useTranscript()
  const [collapsed, setCollapsed] = useState(!showThinking)

  // Follow the global "Show thinking" toggle.
  useEffect(() => {
    setCollapsed(!showThinking)
  }, [showThinking])

  // Follow Expand-all / Collapse-all broadcasts.
  useEffect(() => {
    if (expandSignal.nonce > 0) setCollapsed(!expandSignal.expand)
  }, [expandSignal.nonce, expandSignal.expand])

  return (
    <div className="border border-amber-500/50 rounded-lg p-3 bg-amber-50/10">
      <Button
        variant="ghost"
        size="sm"
        onClick={() => setCollapsed(!collapsed)}
        className="flex items-center gap-1 text-amber-700 hover:text-amber-900"
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4" />
        ) : (
          <ChevronDown className="h-4 w-4" />
        )}
        <Brain className="h-4 w-4" />
        <span className="font-semibold">Thinking</span>
      </Button>

      {!collapsed && (
        <div className="mt-2 text-amber-900 dark:text-amber-200">
          <MarkdownRenderer content={thinking} compact />
        </div>
      )}
    </div>
  )
}
