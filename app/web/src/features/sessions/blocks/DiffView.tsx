import { diffLines } from 'diff'

interface Props {
  oldText: string
  newText: string
}

/**
 * Inline unified diff: removed lines on a red ground, added lines green,
 * context dimmed. Rendered for Edit / MultiEdit tool inputs.
 */
export function DiffView({ oldText, newText }: Props) {
  const parts = diffLines(oldText ?? '', newText ?? '')

  return (
    <pre className="text-xs overflow-x-auto rounded-md border bg-muted/30 leading-relaxed">
      {parts.map((part, i) => {
        const lines = part.value.replace(/\n$/, '').split('\n')
        const marker = part.added ? '+' : part.removed ? '-' : ' '
        const cls = part.added
          ? 'bg-green-500/15 text-green-700 dark:text-green-300'
          : part.removed
            ? 'bg-red-500/15 text-red-700 dark:text-red-300'
            : 'text-muted-foreground'
        return lines.map((line, j) => (
          <div key={`${i}-${j}`} className={`px-2 ${cls}`}>
            <span className="select-none opacity-60 mr-2">{marker}</span>
            {line || ' '}
          </div>
        ))
      })}
    </pre>
  )
}
