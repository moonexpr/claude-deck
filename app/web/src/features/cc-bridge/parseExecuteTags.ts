/**
 * parseExecuteTags — split a model suggestion into typed blocks.
 *
 * The model wraps actionable shell commands in <execute>...</execute> tags.
 * Everything outside the tags is prose; everything inside is a command to run.
 *
 * Rules:
 *   - Tags are matched literally: <execute> … </execute> (case-sensitive, no attrs).
 *   - Unclosed <execute> tags are treated as prose (the opening tag text is kept).
 *   - Multiple consecutive prose segments separated by empty string are not emitted.
 *   - Trailing / leading whitespace is trimmed on execute blocks (but not prose).
 */

export type ParsedBlock =
  | { kind: 'prose'; text: string }
  | { kind: 'execute'; cmd: string }

/**
 * Parse a suggestion string into an ordered list of prose and execute blocks.
 *
 * @param suggestion - raw text from the model
 * @returns array of blocks; never empty (at minimum a single prose block)
 */
export function parseExecuteTags(suggestion: string): ParsedBlock[] {
  const blocks: ParsedBlock[] = []

  // Split on opening tags first, keeping the delimiter.
  const OPEN = '<execute>'
  const CLOSE = '</execute>'

  let remaining = suggestion

  while (remaining.length > 0) {
    const openIdx = remaining.indexOf(OPEN)

    if (openIdx === -1) {
      // No more opening tags — rest is prose.
      if (remaining.length > 0) {
        blocks.push({ kind: 'prose', text: remaining })
      }
      break
    }

    // Prose before the tag
    if (openIdx > 0) {
      blocks.push({ kind: 'prose', text: remaining.slice(0, openIdx) })
    }

    // Advance past the opening tag
    const afterOpen = remaining.slice(openIdx + OPEN.length)
    const closeIdx = afterOpen.indexOf(CLOSE)

    if (closeIdx === -1) {
      // Unclosed tag — treat the remaining text (including the raw open tag) as prose.
      blocks.push({ kind: 'prose', text: OPEN + afterOpen })
      break
    }

    // Execute block
    const cmd = afterOpen.slice(0, closeIdx).trim()
    if (cmd.length > 0) {
      blocks.push({ kind: 'execute', cmd })
    }

    // Advance past the closing tag
    remaining = afterOpen.slice(closeIdx + CLOSE.length)
  }

  // Guarantee at least one block
  if (blocks.length === 0) {
    blocks.push({ kind: 'prose', text: '' })
  }

  return blocks
}
