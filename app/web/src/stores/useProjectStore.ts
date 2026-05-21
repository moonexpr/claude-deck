/**
 * Project store — mirrors frontend/src/contexts/ProjectContext.tsx
 *
 * State shape and actions are a faithful port. `activeProject` is derived
 * (not stored separately): the first project with `is_active === true`.
 *
 * No persist middleware — the legacy context did not persist to localStorage.
 */
import { create } from "zustand";

// ---------------------------------------------------------------------------
// Minimal inline types (no shared lib yet — intentional for A5 scope)
// ---------------------------------------------------------------------------

export interface ProjectResponse {
  id: number;
  name: string;
  path: string;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface ProjectCreate {
  name: string;
  path: string;
}

interface ProjectDiscoveryResponse {
  discovered: ProjectCreate[];
}

interface ProjectListResponse {
  projects: ProjectResponse[];
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const BASE = "/api/v1";

async function apiFetch<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const res = await fetch(`${BASE}/${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

interface ProjectState {
  projects: ProjectResponse[];
  /** Derived: first project where is_active === true */
  activeProject: ProjectResponse | null;
  loading: boolean;
  error: string | null;
}

interface ProjectActions {
  fetchProjects: () => Promise<void>;
  addProject: (projectData: ProjectCreate) => Promise<ProjectResponse>;
  removeProject: (projectId: number) => Promise<void>;
  discoverProjects: (basePath: string) => Promise<ProjectCreate[]>;
  setActiveProject: (projectId: number) => Promise<void>;
  clearActiveProject: () => Promise<void>;
}

type ProjectStore = ProjectState & ProjectActions;

function deriveActive(projects: ProjectResponse[]): ProjectResponse | null {
  return projects.find((p) => p.is_active) ?? null;
}

export const useProjectStore = create<ProjectStore>((set, get) => ({
  // --- state ---
  projects: [],
  activeProject: null,
  loading: false,
  error: null,

  // --- actions ---

  fetchProjects: async () => {
    set({ loading: true, error: null });
    try {
      const data = await apiFetch<ProjectListResponse>("projects");
      set({
        projects: data.projects,
        activeProject: deriveActive(data.projects),
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : "Failed to fetch projects",
      });
    } finally {
      set({ loading: false });
    }
  },

  addProject: async (projectData) => {
    set({ loading: true, error: null });
    try {
      const created = await apiFetch<ProjectResponse>("projects", {
        method: "POST",
        body: JSON.stringify(projectData),
      });
      await get().fetchProjects();
      return created;
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to add project";
      set({ error: message });
      throw err;
    } finally {
      set({ loading: false });
    }
  },

  removeProject: async (projectId) => {
    set({ loading: true, error: null });
    try {
      await apiFetch<void>(`projects/${projectId}`, { method: "DELETE" });
      await get().fetchProjects();
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to remove project";
      set({ error: message });
      throw err;
    } finally {
      set({ loading: false });
    }
  },

  discoverProjects: async (basePath) => {
    set({ loading: true, error: null });
    try {
      const data = await apiFetch<ProjectDiscoveryResponse>(
        "projects/discover",
        {
          method: "POST",
          body: JSON.stringify({ base_path: basePath }),
        },
      );
      return data.discovered;
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to discover projects";
      set({ error: message });
      throw err;
    } finally {
      set({ loading: false });
    }
  },

  setActiveProject: async (projectId) => {
    set({ loading: true, error: null });
    try {
      await apiFetch<void>("projects/active", {
        method: "PUT",
        body: JSON.stringify({ project_id: projectId }),
      });
      await get().fetchProjects();
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to set active project";
      set({ error: message });
      throw err;
    } finally {
      set({ loading: false });
    }
  },

  clearActiveProject: async () => {
    set({ loading: true, error: null });
    try {
      await apiFetch<void>("projects/active", { method: "DELETE" });
      await get().fetchProjects();
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to clear active project";
      set({ error: message });
      throw err;
    } finally {
      set({ loading: false });
    }
  },
}));
