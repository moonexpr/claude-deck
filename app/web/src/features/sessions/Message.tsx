import { useState } from 'react'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Copy, Check, Wrench } from 'lucide-react'
import { ContentBlockRenderer } from './ContentBlockRenderer'
import { formatDistanceToNow } from './dateUtils'
import type { SessionMessage, ContentBlock } from '@/types/sessions'

interface Props {
  message: SessionMessage
}

// Helper to extract total from token number or records
const sumTokens = (tokens: number | Record<string, number> | undefined) => {
  if (!tokens) return 0
  if (typeof tokens === 'number') return tokens
  return Object.values(tokens).reduce((acc, v) => acc + v, 0)
}

/** Plain-text rendering of a message's blocks for clipboard copy. */
function blocksToText(blocks: ContentBlock[]): string {
  return blocks
    .map((b) => {
      switch (b.type) {
        case 'text':
          return b.text ?? ''
        case 'thinking':
          return `[thinking]\n${b.thinking ?? ''}`
        case 'tool_use':
          return `[tool: ${b.name}]\n${JSON.stringify(b.input ?? {}, null, 2)}`
        case 'tool_result':
          return `[result]\n${typeof b.content === 'string' ? b.content : JSON.stringify(b.content)}`
        default:
          return ''
      }
    })
    .filter(Boolean)
    .join('\n\n')
}

export function Message({ message }: Props) {
  const isUser = message.type === 'user'
  const timeAgo = formatDistanceToNow(new Date(message.timestamp))
  const [copied, setCopied] = useState(false)

  const toolCount = message.content.filter((b) => b.type === 'tool_use').length

  const totalTokens = message.usage
    ? sumTokens(message.usage.input_tokens as number | Record<string, number> | undefined) +
      sumTokens(message.usage.output_tokens as number | Record<string, number> | undefined) +
      sumTokens(message.usage.cache_creation_input_tokens as number | Record<string, number> | undefined) +
      sumTokens(message.usage.cache_read_input_tokens as number | Record<string, number> | undefined)
    : null

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation()
    navigator.clipboard.writeText(blocksToText(message.content)).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    })
  }

  return (
    <Card
      className={`p-4 ${isUser ? 'border-blue-500/50 bg-blue-50/10' : 'border-gray-500/50 bg-gray-50/10'}`}
    >
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <Badge variant={isUser ? 'default' : 'secondary'}>
            {isUser ? 'User' : 'Assistant'}
          </Badge>
          {message.model && (
            <Badge variant="outline" className="text-xs">
              {message.model}
            </Badge>
          )}
          {toolCount > 0 && (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              <Wrench className="h-3 w-3" />
              {toolCount} {toolCount === 1 ? 'tool' : 'tools'}
            </span>
          )}
          <span className="text-xs text-muted-foreground">{timeAgo}</span>
        </div>

        <div className="flex items-center gap-2">
          {totalTokens != null && totalTokens > 0 && (
            <Badge variant="outline" className="text-xs">
              {totalTokens.toLocaleString()} tokens
            </Badge>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleCopy}
            className="h-7 px-2"
            title="Copy message text"
          >
            {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          </Button>
        </div>
      </div>

      {/* Content Blocks */}
      <div className="space-y-3">
        {message.content.map((block, idx) => (
          <ContentBlockRenderer key={idx} block={block} />
        ))}
      </div>

      {/* Token Usage Detail (if available) */}
      {message.usage && (
        <div className="mt-3 pt-3 border-t text-xs text-muted-foreground flex gap-4 flex-wrap">
          {message.usage.input_tokens !== undefined && (
            <span>Input: {sumTokens(message.usage.input_tokens as number | Record<string, number> | undefined)}</span>
          )}
          {message.usage.output_tokens !== undefined && (
            <span>Output: {sumTokens(message.usage.output_tokens as number | Record<string, number> | undefined)}</span>
          )}
          {message.usage.cache_creation_input_tokens !== undefined && (
            <span>Cache Create: {sumTokens(message.usage.cache_creation_input_tokens as number | Record<string, number> | undefined)}</span>
          )}
          {message.usage.cache_read_input_tokens !== undefined && (
            <span>Cache Read: {sumTokens(message.usage.cache_read_input_tokens as number | Record<string, number> | undefined)}</span>
          )}
        </div>
      )}
    </Card>
  )
}
