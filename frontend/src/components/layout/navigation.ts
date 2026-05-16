import {
  LayoutDashboard,
  Settings,
  Server,
  Terminal,
  Package,
  Webhook,
  Shield,
  Bot,
  Sparkles,
  Brain,
  Paintbrush,
  Activity,
  MessageSquare,
  BarChart3,
  FolderOpen,
  Archive,
  Gauge,
  ClipboardList,
  MonitorPlay,
  Radio,
  type LucideIcon,
} from 'lucide-react'

export type NavigationItem = { name: string; href: string; icon: LucideIcon }

export const navigation: NavigationItem[] = [
  // Tier 1: Overview & Setup
  { name: 'Dashboard', href: '/', icon: LayoutDashboard },
  { name: 'Projects', href: '/projects', icon: FolderOpen },
  { name: 'Config', href: '/config', icon: Settings },

  // Tier 2: Core Configuration Sections
  { name: 'MCP Servers', href: '/mcp', icon: Server },
  { name: 'Commands', href: '/commands', icon: Terminal },
  { name: 'Plugins', href: '/plugins', icon: Package },
  { name: 'Hooks', href: '/hooks', icon: Webhook },
  { name: 'Permissions', href: '/permissions', icon: Shield },
  { name: 'Agents', href: '/agents', icon: Bot },
  { name: 'Skills', href: '/skills', icon: Sparkles },
  { name: 'Memory', href: '/memory', icon: Brain },

  // Tier 3: Customization
  { name: 'Output Styles', href: '/output-styles', icon: Paintbrush },
  { name: 'Status Line', href: '/statusline', icon: Activity },

  // Tier 4: Monitoring & Tools
  { name: 'Sessions', href: '/sessions', icon: MessageSquare },
  { name: 'Presence', href: '/presence', icon: Radio },
  { name: 'CC Bridge', href: '/cc-bridge', icon: MonitorPlay },
  { name: 'Plans', href: '/plans', icon: ClipboardList },
  { name: 'Context', href: '/context', icon: Gauge },
  { name: 'Usage', href: '/usage', icon: BarChart3 },
  { name: 'Backup', href: '/backup', icon: Archive },
]
