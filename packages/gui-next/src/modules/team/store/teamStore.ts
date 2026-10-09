import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { call, hasTauri } from '../../../ipc'
import type { SubagentModelOverride } from '../../settings/lib/subagentProfiles'

/** Mirrors `agent::team::TeamAgent` (`rename_all = "camelCase"`). */
export interface TeamAgent {
  id: string
  name: string
  role: string
  status: 'active' | 'proposed'
  reportsTo: string | null
  model: SubagentModelOverride | null
  /** `null` = full tool access. */
  allowedTools: string[] | null
  owns: string[]
  color: string | null
  created: string | null
  /** `user` or `master`. */
  proposedBy: string | null
  reason: string | null
  instructions: string
  /** The user's comments on its past runs — what it "remembers". */
  feedback: Feedback[]
  path: string
}

/** Mirrors `agent::team::Feedback`. */
export interface Feedback {
  date: string
  text: string
  task: string | null
}

/** Mirrors `agent::runs::RunComment`. */
export interface RunComment {
  text: string
  at: number
  duringRun: boolean
}

/** Mirrors `agent::runs::RunMeta`. */
export interface AgentRun {
  id: string
  root: string
  agentId: string
  agentName: string
  description: string
  prompt: string
  status: 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted'
  startedAt: number
  endedAt: number | null
  output: string | null
  turns: number
  toolCalls: number
  comments: RunComment[]
  /** Subagents it spawned during the run (`agent::runs::ChildRun`). */
  children?: ChildRun[]
}

/** Mirrors `agent::runs::ChildRun`. */
export interface ChildRun {
  id: string
  kind: string
  description: string
  status: 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted'
  startedAt: number
  endedAt: number | null
}

const runKey = (root: string, agentId: string) => `${root}::${agentId}`

interface TeamState {
  /** Project folder → its specialists. Absent = not loaded yet. */
  byRoot: Record<string, TeamAgent[]>
  /** `root::agentId` → that agent's runs, newest first. */
  runs: Record<string, AgentRun[]>
  /** The agent whose page the Agents panel shows, if any. */
  focused: { root: string; agentId: string } | null
  focus: (root: string, agentId: string | null) => void
  loadRuns: (root: string, agentId: string) => Promise<AgentRun[]>
  runsOf: (root: string, agentId: string) => AgentRun[] | undefined
  /** Comment on a run: steers it if live, and is remembered either way. */
  comment: (run: AgentRun, text: string) => Promise<void>
  load: (root: string) => Promise<void>
  save: (root: string, agent: TeamAgent, originalId: string | null) => Promise<TeamAgent>
  approve: (root: string, id: string) => Promise<void>
  remove: (root: string, id: string) => Promise<void>
}

export const useTeamStore = create<TeamState>((set, get) => ({
  byRoot: {},
  runs: {},
  focused: null,
  focus: (root, agentId) => set({ focused: agentId ? { root, agentId } : null }),
  runsOf: (root, agentId) => get().runs[runKey(root, agentId)],
  loadRuns: async (root, agentId) => {
    const runs = (await call<AgentRun[]>('cmd_list_agent_runs', { root, agentId }).catch(() => null)) ?? []
    set((s) => ({ runs: { ...s.runs, [runKey(root, agentId)]: runs } }))
    return runs
  },
  comment: async (run, text) => {
    await call('cmd_comment_on_run', { id: run.id, text })
    await Promise.all([get().loadRuns(run.root, run.agentId), get().load(run.root)])
  },
  load: async (root) => {
    const agents = (await call<TeamAgent[]>('cmd_list_team', { root }).catch(() => null)) ?? []
    set((s) => ({ byRoot: { ...s.byRoot, [root]: agents } }))
    // Enough to show who is working right now, wherever the team is listed.
    await Promise.all(agents.filter((a) => a.status === 'active').map((a) => get().loadRuns(root, a.id)))
  },
  save: async (root, agent, originalId) => {
    const saved = await call<TeamAgent>('cmd_save_team_agent', { root, agent, originalId })
    if (!saved) throw new Error('The desktop backend is not connected.')
    await get().load(root)
    return saved
  },
  approve: async (root, id) => {
    await call('cmd_approve_team_agent', { root, id })
    await get().load(root)
  },
  remove: async (root, id) => {
    await call('cmd_delete_team_agent', { root, id })
    await get().load(root)
  },
}))

// The master proposes hires from inside a turn; the backend announces every
// change so an open panel (and the sidebar's pending count) update at once.
if (hasTauri()) {
  void listen<{ root: string }>('team:changed', (e) => {
    const root = e.payload?.root
    if (root && root in useTeamStore.getState().byRoot) void useTeamStore.getState().load(root)
  })
  // A run started, finished or was commented on.
  void listen<{ root: string; agentId: string }>('team:run', (e) => {
    const { root, agentId } = e.payload ?? {}
    if (root && agentId) void useTeamStore.getState().loadRuns(root, agentId)
  })
}

/** Whether any of this agent's runs is working right now. */
export function isWorking(runs: AgentRun[] | undefined): boolean {
  return !!runs?.some((r) => r.status === 'running')
}

export function blankAgent(): TeamAgent {
  return {
    id: '',
    name: '',
    role: '',
    status: 'active',
    reportsTo: null,
    model: null,
    allowedTools: null,
    owns: [],
    color: null,
    created: null,
    proposedBy: null,
    reason: null,
    instructions: '',
    feedback: [],
    path: '',
  }
}
