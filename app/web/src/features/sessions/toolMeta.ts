import {
  Terminal,
  FileText,
  FilePen,
  FileDiff,
  Search,
  FolderSearch,
  FolderTree,
  ListTodo,
  Bot,
  Globe,
  Notebook,
  Wrench,
  type LucideIcon,
} from 'lucide-react'

/** Map a file path's extension to a Prism language id. */
export function languageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    jsx: 'jsx',
    py: 'python',
    rs: 'rust',
    go: 'go',
    rb: 'ruby',
    java: 'java',
    c: 'c',
    h: 'c',
    cpp: 'cpp',
    cc: 'cpp',
    cs: 'csharp',
    swift: 'swift',
    kt: 'kotlin',
    sh: 'bash',
    bash: 'bash',
    zsh: 'bash',
    fish: 'bash',
    json: 'json',
    yaml: 'yaml',
    yml: 'yaml',
    toml: 'toml',
    md: 'markdown',
    html: 'markup',
    css: 'css',
    scss: 'scss',
    sql: 'sql',
    lua: 'lua',
    php: 'php',
  }
  return map[ext] ?? 'text'
}

/** Icon + accent color (Tailwind border/text hue) per tool name. */
export interface ToolStyle {
  icon: LucideIcon
  hue: string
}

const TOOL_STYLES: Record<string, ToolStyle> = {
  Bash: { icon: Terminal, hue: 'purple' },
  Read: { icon: FileText, hue: 'sky' },
  Write: { icon: FilePen, hue: 'blue' },
  Edit: { icon: FileDiff, hue: 'blue' },
  MultiEdit: { icon: FileDiff, hue: 'blue' },
  Grep: { icon: Search, hue: 'teal' },
  Glob: { icon: FolderSearch, hue: 'teal' },
  LS: { icon: FolderTree, hue: 'teal' },
  TodoWrite: { icon: ListTodo, hue: 'amber' },
  Task: { icon: Bot, hue: 'fuchsia' },
  Agent: { icon: Bot, hue: 'fuchsia' },
  WebFetch: { icon: Globe, hue: 'cyan' },
  WebSearch: { icon: Globe, hue: 'cyan' },
  NotebookEdit: { icon: Notebook, hue: 'blue' },
}

export function toolStyle(name: string): ToolStyle {
  return TOOL_STYLES[name] ?? { icon: Wrench, hue: 'gray' }
}
