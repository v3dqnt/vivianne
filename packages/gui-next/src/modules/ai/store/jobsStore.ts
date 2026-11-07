import { create } from 'zustand'
import { call } from '../../../ipc'

/** Mirrors `agent::tools::background::Job` (camelCase). */
export interface Job {
  callId: string
  tool: string
  label: string
  thread: string | null
  startedAtMs: number
  status: 'running' | 'finished' | 'failed' | 'stopped'
  outputTail: string | null
}

interface JobsState {
  jobs: Job[]
  refresh: (threadId?: string | null) => Promise<void>
  stop: (callId: string) => Promise<void>
  clearEnded: () => Promise<void>
}

export const useJobsStore = create<JobsState>((set, get) => ({
  jobs: [],
  refresh: async (threadId) => {
    set({ jobs: (await call<Job[]>('cmd_jobs_list', { threadId: threadId ?? null })) ?? [] })
  },
  stop: async (callId) => {
    await call('cmd_job_stop', { callId })
    await get().refresh()
  },
  clearEnded: async () => {
    await call('cmd_jobs_clear')
    await get().refresh()
  },
}))
