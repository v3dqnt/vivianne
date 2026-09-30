import { create } from 'zustand'
import { call, on, hasTauri } from '../../../ipc'

/** Mirrors `agent::team_run::Member`. */
export interface TeamRunMember {
  id: string
  name: string
  role: string
  color: string | null
  status: 'starting' | 'working' | 'done' | 'failed' | 'stopped'
  taskId: string | null
  output: string | null
}

/** Mirrors `agent::team_run::TeamMessage`. */
export interface TeamRunMessage {
  from: string
  to: string
  text: string
  at: number
}

/** Mirrors `agent::team_run::TeamRun`. */
export interface TeamRun {
  id: string
  task: string
  members: TeamRunMember[]
  messages: TeamRunMessage[]
  status: 'running' | 'done' | 'stopped'
  startedAt: number
}

interface TeamRunState {
  runs: Record<string, TeamRun>
  /** The run started most recently — what a still-running `team_run` call
   * shows before its result names its run. */
  latest: string | null
  fetch: (id: string) => Promise<void>
}

export const useTeamRunStore = create<TeamRunState>((set) => ({
  runs: {},
  latest: null,
  fetch: async (id) => {
    const run = await call<TeamRun | null>('cmd_team_run_get', { id })
    if (run) set((s) => ({ runs: { ...s.runs, [run.id]: run } }))
  },
}))

// Every change to a run is pushed as a whole `TeamRun`.
if (hasTauri()) {
  void on<TeamRun>('team_update', (run) => {
    useTeamRunStore.setState((s) => ({
      runs: { ...s.runs, [run.id]: run },
      latest: !s.latest || s.runs[s.latest]?.startedAt <= run.startedAt ? run.id : s.latest,
    }))
  })
}
