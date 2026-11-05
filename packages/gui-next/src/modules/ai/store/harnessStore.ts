import { create } from 'zustand'
import { call, on } from '../../../ipc'

/** A coding agent Vivianne can run through ACP (`cmd_harness_list`). */
export interface Harness {
  id: string
  name: string
  description: string
  icon?: string | null
  /** The launcher (npx / uvx) is on this computer. */
  available: boolean
  /** Its package has been fetched and started here before. */
  ready: boolean
}

export interface DebugStep {
  name: string
  ok: boolean
  ms: number
  detail: string
}
export interface DebugReport {
  steps: DebugStep[]
  reply: string
  trace: string[]
}

export type HarnessStatus = 'ready' | 'setup' | 'missing'
export const statusOf = (h: Harness): HarnessStatus => (!h.available ? 'missing' : h.ready ? 'ready' : 'setup')

interface HarnessState {
  list: Harness[] | null
  error: boolean
  /** What each agent is doing now: setting up or being tested. */
  busy: Record<string, 'install' | 'debug'>
  failures: Record<string, string>
  /** What an install is doing: the last thing npm fetched, and how much is on disk. */
  progress: Record<string, { line: string; bytes: number }>
  reports: Record<string, DebugReport>
  refresh: () => Promise<void>
  install: (id: string) => Promise<void>
  debug: (id: string) => Promise<void>
}

let listening = false

export const useHarnessStore = create<HarnessState>((set, get) => {
  const run = async (id: string, kind: 'install' | 'debug', work: () => Promise<void>) => {
    set((s) => ({ busy: { ...s.busy, [id]: kind }, failures: { ...s.failures, [id]: '' } }))
    try {
      await work()
    } catch (e) {
      set((s) => ({ failures: { ...s.failures, [id]: String(e) } }))
    } finally {
      set((s) => {
        const { [id]: _done, ...busy } = s.busy
        return { busy }
      })
      await get().refresh()
    }
  }
  return {
    list: null,
    error: false,
    busy: {},
    failures: {},
    progress: {},
    reports: {},
    refresh: async () => {
      if (!listening) {
        listening = true
        void on<{ id: string; line: string; bytes: number }>('harness:install', (p) => set((s) => ({ progress: { ...s.progress, [p.id]: { line: p.line, bytes: p.bytes } } })))
      }
      try {
        set({ list: await call<Harness[]>('cmd_harness_list'), error: false })
      } catch {
        set((s) => ({ list: s.list ?? [], error: true }))
      }
    },
    install: (id) => run(id, 'install', async () => void (await call<string>('cmd_harness_install', { id }))),
    debug: (id) =>
      run(id, 'debug', async () => {
        const report = await call<DebugReport>('cmd_harness_debug', { id })
        if (report) set((s) => ({ reports: { ...s.reports, [id]: report } }))
      }),
  }
})
