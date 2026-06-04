import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { CodeHighlight } from './CodeHighlight'

interface MarkdownRendererProps {
  content: string
  className?: string
  /** Tighter prose spacing — used for inline contexts like thinking blocks. */
  compact?: boolean
}

export function MarkdownRenderer({ content, className = '', compact = false }: MarkdownRendererProps) {
  const prose = compact
    ? 'prose prose-sm max-w-none dark:prose-invert prose-p:my-1 prose-pre:my-1 prose-headings:my-1 prose-ul:my-1 prose-ol:my-1'
    : 'prose prose-sm max-w-none dark:prose-invert'

  return (
    <div className={`${prose} ${className}`}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { className: codeClassName, children } = props
            const match = /language-(\w+)/.exec(codeClassName || '')
            if (match) {
              return (
                <CodeHighlight
                  code={String(children).replace(/\n$/, '')}
                  language={match[1]}
                />
              )
            }
            return <code className={codeClassName}>{children}</code>
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  )
}
