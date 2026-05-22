/**
 * Dashboard store — mirrors frontend/src/contexts/DashboardContext.tsx
 *
 * Fetches are lazy: `refreshDashboard` must be called explicitly (e.g. from
 * the dashboard page with `autoFetch`). The store also re-fetches when the
 * active project changes — callers subscribe to `activeProject` from
 * useProjectStore and call `refreshDashboard` on change (see MainLayout).
 *
 * The 12-endpoint fan-out is replicated with inline fetch calls against the
 * Vite-proxied /api/v1/ paths. No shared lib/api.ts — that is a later task.
 *
 * No persist middleware — the legacy context did not persist to localStorage.
 */
import { create } from "zustand";

// ---------------------------------------------------------------------------
// Exported stats shape (mirrors DashboardContext.DashboardStats)
// ---------------------------------------------------------------------------

export interface DashboardStats {
  mcpServerCount: number;
  commandCount: number;
  agentCount: number;
  skillCount: number;
  hookCount: number;
  pluginCount: number;
  permissionCount: number;
  outputStyleCount: number;
  allowRules: number;
  denyRules: number;
  settingsKeys: number;
  sessionCount: number;
  sessionsToday: number;
  sessionsThisWeek: number;
  mostActiveProject?: string;
  totalMessages: number;
  contextHighestPct: number;
  contextHighestProject?: string;
  contextActiveCount: number;
  planCount: number;
}

// ---------------------------------------------------------------------------
// Minimal response-shape types (enough to compute stats — no excess fields)
// ---------------------------------------------------------------------------

interface PermissionRule {
  type: "allow" | "deny";
}

interface SessionStatsResponse {
  total_sessions: number;
  sessions_today: number;
  sessions_this_week: number;
  most_active_project?: string;
  total_messages: number;
}

interface ActiveSessionContext {
  is_active: boolean;
  context_percentage: number;
  project_name?: string;
}

interface ActiveSessionsResponse {
  sessions: ActiveSessionContext[];
}

interface PlanStatsResponse {
  total_plans: number;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const BASE = "/api/v1";

async function apiFetch<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}/${path}`, {
    headers: { "Content-Type": "application/json" },
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

function buildEndpoint(resource: string, projectPath?: string): string {
  if (projectPath) {
    return `${resource}?project_path=${encodeURIComponent(projectPath)}`;
  }
  return resource;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

// Module-scoped sequence counter — mirrors the legacy DashboardContext's
// fetchId ref. Incremented on every refreshDashboard call so that a slower
// earlier fetch can detect it has been superseded and discard its result.
let refreshSeq = 0;

interface DashboardState {
  stats: DashboardStats | null;
  loading: boolean;
  error: string | null;
  lastFetched: Date | null;
}

interface DashboardActions {
  refreshDashboard: (projectPath?: string) => Promise<void>;
}

type DashboardStore = DashboardState & DashboardActions;

export const useDashboardStore = create<DashboardStore>((set) => ({
  // --- state ---
  stats: null,
  loading: false,
  error: null,
  lastFetched: null,

  // --- actions ---

  refreshDashboard: async (projectPath?: string) => {
    const seq = ++refreshSeq;
    set({ loading: true, error: null });
    try {
      const ep = (r: string) => buildEndpoint(r, projectPath);

      const [
        configData,
        mcpData,
        agentsData,
        skillsData,
        pluginsData,
        hooksData,
        permissionsData,
        commandsData,
        outputStylesData,
        sessionStatsData,
        contextData,
        planStatsData,
      ] = await Promise.all([
        apiFetch<{ settings?: Record<string, unknown> }>(ep("config")),
        apiFetch<{ servers: unknown[] }>(ep("mcp/servers")),
        apiFetch<{ agents: unknown[] }>(ep("agents")),
        apiFetch<{ skills: unknown[] }>(ep("agents/skills")),
        apiFetch<{ plugins: unknown[] }>(ep("plugins")),
        apiFetch<{ hooks: unknown[] }>(ep("hooks")),
        apiFetch<{ rules: PermissionRule[] }>(ep("permissions")),
        apiFetch<{ commands: unknown[] }>(ep("commands")),
        apiFetch<{ output_styles?: unknown[] }>(ep("output-styles")),
        apiFetch<SessionStatsResponse>("sessions/dashboard/stats"),
        apiFetch<ActiveSessionsResponse>("context/active").catch(
          (): ActiveSessionsResponse => ({ sessions: [] }),
        ),
        apiFetch<PlanStatsResponse>("plans/stats").catch(
          (): PlanStatsResponse => ({ total_plans: 0 }),
        ),
      ]);

      // Superseded by a later call — discard stale result
      if (seq !== refreshSeq) return;

      const allowRules = permissionsData.rules.filter(
        (r) => r.type === "allow",
      ).length;
      const denyRules = permissionsData.rules.filter(
        (r) => r.type === "deny",
      ).length;

      const activeSessions = contextData.sessions.filter((s) => s.is_active);
      const highestCtx =
        contextData.sessions.length > 0
          ? contextData.sessions.reduce((max, s) =>
              s.context_percentage > max.context_percentage ? s : max,
              contextData.sessions[0],
            )
          : null;

      set({
        stats: {
          mcpServerCount: mcpData.servers.length,
          commandCount: commandsData.commands.length,
          agentCount: agentsData.agents.length,
          skillCount: skillsData.skills.length,
          hookCount: hooksData.hooks.length,
          pluginCount: pluginsData.plugins.length,
          permissionCount: allowRules + denyRules,
          outputStyleCount: outputStylesData.output_styles?.length ?? 0,
          allowRules,
          denyRules,
          settingsKeys: Object.keys(configData.settings ?? {}).length,
          sessionCount: sessionStatsData.total_sessions,
          sessionsToday: sessionStatsData.sessions_today,
          sessionsThisWeek: sessionStatsData.sessions_this_week,
          mostActiveProject: sessionStatsData.most_active_project,
          totalMessages: sessionStatsData.total_messages,
          contextHighestPct: highestCtx?.context_percentage ?? 0,
          contextHighestProject: highestCtx?.project_name,
          contextActiveCount: activeSessions.length,
          planCount: planStatsData.total_plans,
        },
        lastFetched: new Date(),
      });
    } catch (err) {
      // Superseded — don't surface a stale error
      if (seq !== refreshSeq) return;
      set({
        error:
          err instanceof Error ? err.message : "Failed to load dashboard data",
      });
    } finally {
      // Only the latest call clears loading
      if (seq === refreshSeq) set({ loading: false });
    }
  },
}));
