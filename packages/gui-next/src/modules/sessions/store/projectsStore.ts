import { create } from 'zustand'
import { call } from '../../../ipc'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'

export interface Project {
  id: string
  name: string
  created_at: string
  path: string | null
  color: string | null
}

/** Accent colors for the sidebar project rail. */
export const PROJECT_COLORS: Record<string, string> = {
  violet: '#8b5cf6',
  lime: '#84cc16',
  teal: '#14b8a6',
  magenta: '#d946a6',
  amber: '#f5b942',
  blue: '#4a9eff',
  red: '#e5484d',
  cyan: '#22d3ee',
}

const COLOR_IDS = Object.keys(PROJECT_COLORS)

/** A project with no explicit color still needs a stable one — derive it
 * from the id so it never changes between launches. */
export function projectColor(project: Pick<Project, 'id' | 'color'>): string {
  if (project.color && project.color in PROJECT_COLORS) return PROJECT_COLORS[project.color]
  let hash = 0
  for (let i = 0; i < project.id.length; i++) hash = (hash * 31 + project.id.charCodeAt(i)) | 0
  return PROJECT_COLORS[COLOR_IDS[Math.abs(hash) % COLOR_IDS.length]]
}

/** Two-letter monogram for the rail square. */
export function projectInitial(name: string): string {
  return (name.trim()[0] ?? '?').toUpperCase()
}

interface ProjectsStoreState {
  projects: Project[]
  /** sessionId → projectId (from `cmd_get_session_project_map`). */
  sessionProjectMap: Record<string, string>
  /** Which project the sidebar is currently showing. */
  activeProjectId: string | null
  loading: boolean
  refresh: () => Promise<void>
  create: (name: string, path?: string | null, color?: string | null) => Promise<Project | null>
  update: (id: string, name: string, path?: string | null, color?: string | null) => Promise<void>
  remove: (id: string) => Promise<void>
  /** Assign (or clear, with null) a session's project. */
  assign: (sessionId: string, projectId: string | null) => Promise<void>
  setActiveProject: (id: string | null) => void
  /** Open the active project's folder in Explorer or a terminal. */
  openExternal: (target: 'explorer' | 'terminal') => Promise<void>
}

/** The project whose folder the workspace was last synced to. Guards against
 * re-applying on every `refresh()` (which runs after any assign/rename), so
 * folders the user added on top of the project's own are only discarded when
 * they actually switch projects. */
let lastSyncedProjectId: string | null = null

/**
 * A project *is* its folder: selecting one points the agent's workspace at
 * it. Extra folders stay addable from the workspace selector and persist
 * until the next project switch, which resets to that project's folder.
 */
function syncWorkspaceToProject(project: Project | undefined) {
  if (!project || project.id === lastSyncedProjectId) return
  lastSyncedProjectId = project.id
  if (!project.path) return
  const ws = useWorkspaceStore.getState()
  if (ws.folders[0] === project.path) return
  void ws.setWorkspace([project.path], ws.accessMode)
}

export const useProjectsStore = create<ProjectsStoreState>((set, get) => ({
  projects: [],
  sessionProjectMap: {},
  activeProjectId: null,
  loading: false,

  refresh: async () => {
    set({ loading: true })
    try {
      const [projects, map] = await Promise.all([
        call<Project[]>('cmd_list_projects'),
        call<Record<string, string>>('cmd_get_session_project_map'),
      ])
      const list = projects ?? []
      set((s) => ({
        projects: list,
        sessionProjectMap: map ?? {},
        // Keep a valid selection at all times — the sidebar always shows
        // exactly one project, so a null/stale id would render it empty.
        activeProjectId: list.some((p) => p.id === s.activeProjectId) ? s.activeProjectId : (list[0]?.id ?? null),
      }))
      // Covers app start, where the first project resolves here rather than
      // through an explicit click.
      syncWorkspaceToProject(list.find((p) => p.id === get().activeProjectId))
    } finally {
      set({ loading: false })
    }
  },

  create: async (name, path = null, color = null) => {
    const trimmed = name.trim()
    if (!trimmed) return null
    const project = await call<Project>('cmd_create_project', { name: trimmed, path, color })
    await get().refresh()
    // Via the action, not a raw `set` — a new project should point the
    // workspace at its folder straight away.
    if (project) get().setActiveProject(project.id)
    return project
  },

  update: async (id, name, path = null, color = null) => {
    const trimmed = name.trim()
    if (!trimmed) return
    await call('cmd_update_project', { id, name: trimmed, path, color })
    // Re-point the workspace if this project's folder just changed.
    if (id === lastSyncedProjectId) lastSyncedProjectId = null
    await get().refresh()
  },

  remove: async (id) => {
    await call('cmd_delete_project', { id })
    await get().refresh()
  },

  assign: async (sessionId, projectId) => {
    await call('cmd_assign_session_project', { sessionId, projectId })
    await get().refresh()
  },

  setActiveProject: (id) => {
    set({ activeProjectId: id })
    syncWorkspaceToProject(get().projects.find((p) => p.id === id))
  },

  openExternal: async (target) => {
    const { projects, activeProjectId } = get()
    const path = projects.find((p) => p.id === activeProjectId)?.path
    if (!path) return
    await call('cmd_open_external', { path, target })
  },
}))
