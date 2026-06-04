import { lazy, Suspense } from 'react'
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism'
import type { CSSProperties } from 'react'

/**
 * Lazy-loaded Prism highlighter. The react-syntax-highlighter core +
 * language grammars are pulled only when a code block actually renders,
 * keeping text-only transcripts off the highlighter bundle. The oneDark
 * style is a small standalone object, so it's imported eagerly for the
 * fallback-free first paint.
 */
const Prism = lazy(() =>
  import('react-syntax-highlighter').then((m) => ({ default: m.Prism }))
)

interface Props {
  code: string
  language: string
  className?: string
}

export function CodeHighlight({ code, language, className }: Props) {
  return (
    <Suspense
      fallback={
        <pre className={`text-xs overflow-x-auto p-2 ${className ?? ''}`}>{code}</pre>
      }
    >
      <Prism
        style={oneDark as { [key: string]: CSSProperties }}
        language={language}
        PreTag="div"
        customStyle={{ margin: 0, fontSize: '0.75rem' }}
      >
        {code}
      </Prism>
    </Suspense>
  )
}
