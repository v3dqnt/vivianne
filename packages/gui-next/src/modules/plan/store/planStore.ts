import { create } from 'zustand'
import { call, on } from '../../../ipc'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { usePanelStore } from '../../dock/store/panelStore'

/** `agent::plans::PlanSection` / `PlanStep` / `Plan` — camelCase on the wire. */
export interface PlanSection {
  id: string
  heading: string
  body: string
}
export interface PlanStep {
  id: string
  title: string
  detail: string
  owner: string | null
  dependsOn: string[]
}
export interface Plan {
  id: string
  version: number
  title: string
  summary: string
  sections: PlanSection[]
  steps: PlanStep[]
  changes: string | null
  createdAt: number
}
export interface StepState {
  owner: string | null
  status: 'pending' | 'running' | 'done' | 'failed' | 'skipped'
  taskId: string | null
  output: string | null
  startedAt: number | null
  endedAt: number | null
}
export type PlanStatus = 'review' | 'approved' | 'executing' | 'done' | 'failed' | 'cancelled'
export interface PlanState {
  id: string
  status: PlanStatus
  latest: number
  approvedVersion: number | null
  steps: Record<string, StepState>
  updatedAt: number
}
export interface PlanView {
  plan: Plan
  versions: number[]
  state: PlanState
}
export interface PlanSummary {
  id: string
  title: string
  latest: number
  status: PlanStatus
  updatedAt: number
}

/** A reviewer's note on one part of a plan. `quote` is the text they
 * selected, when they commented on a passage rather than a whole part. */
export interface PlanComment {
  targetId: string
  /** How the part reads in the feedback: "Approach", "Step 2: API routes". */
  targetLabel: string
  quote: string | null
  text: string
}

interface PlanStoreState {
  /** Plan mode for the conversation: research only, ending in a plan. */
  planMode: boolean
  refreshMode: () => Promise<void>
  setPlanMode: (on: boolean) => Promise<void>

  plans: PlanSummary[]
  /** The plan open in the panel, at the version being viewed. */
  current: PlanView | null
  /** The version before the one viewed, for marking what changed. */
  previous: Plan | null
  /** Unsent comments, per plan id. */
  comments: Record<string, PlanComment[]>
  busy: boolean
  error: string | null

  init: () => Promise<void>
  refreshList: () => Promise<void>
  open: (id: string, version?: number) => Promise<void>
  addComment: (comment: PlanComment) => void
  removeComment: (index: number) => void
  requestChanges: () => Promise<void>
  approve: () => Promise<void>
  cancelExecution: () => Promise<void>
}

const root = () => useWorkspaceStore.getState().folders[0] ?? null

/** The message that carries review comments back to the agent. */
export function feedbackMessage(plan: Plan, comments: PlanComment[]): string {
  const lines = comments.map((c) => {
    const quote = c.quote ? ` (on: "${c.quote.replace(/\s+/g, ' ').slice(0, 240)}")` : ''
    return `- ${c.targetLabel} [${c.targetId}]${quote}: ${c.text.trim()}`
  })
  return `<plan_feedback id="${plan.id}" version="${plan.version}">\n${lines.join('\n')}\n</plan_feedback>\nRevise the plan to address every comment above, then submit the whole revised plan with submit_plan (plan_id "${plan.id}"), saying in 'changes' what you changed.`
}

let listening = false

export const usePlanStore = create<PlanStoreState>((set, get) => ({
  planMode: false,
  plans: [],
  current: null,
  previous: null,
  comments: {},
  busy: false,
  error: null,

  refreshMode: async () => {
    set({ planMode: (await call<boolean>('cmd_plan_mode_get', { threadId: '' })) ?? false })
  },
  setPlanMode: async (on) => {
    set({ planMode: on })
    await call('cmd_plan_mode_set', { threadId: '', on })
  },

  init: async () => {
    await get().refreshMode()
    await get().refreshList()
    if (listening) return
    listening = true
    await on<{ root: string; id: string }>('plan_updated', async ({ root: r, id }) => {
      if (r !== root()) return
      await get().refreshList()
      const cur = get().current
      // A plan just submitted (or revised) opens at its newest version; one
      // being carried out refreshes in place.
      const latest = get().plans.find((p) => p.id === id)
      if (!cur || cur.plan.id !== id || latest?.status === 'review') {
        await get().open(id)
        usePanelStore.getState().openRight('plan')
      } else {
        await get().open(id, cur.plan.version)
      }
    })
  },

  refreshList: async () => {
    const r = root()
    set({ plans: r ? ((await call<PlanSummary[]>('cmd_plan_list', { root: r })) ?? []) : [] })
  },

  open: async (id, version) => {
    const r = root()
    if (!r) return
    const view = await call<PlanView | null>('cmd_plan_get', { root: r, id, version: version ?? null })
    if (!view) return
    const prevVersion = [...view.versions].reverse().find((v) => v < view.plan.version)
    const previous =
      prevVersion !== undefined ? ((await call<PlanView | null>('cmd_plan_get', { root: r, id, version: prevVersion }))?.plan ?? null) : null
    set({ current: view, previous, error: null })
  },

  addComment: (comment) => {
    const id = get().current?.plan.id
    if (!id) return
    set((s) => ({ comments: { ...s.comments, [id]: [...(s.comments[id] ?? []), comment] } }))
  },
  removeComment: (index) => {
    const id = get().current?.plan.id
    if (!id) return
    set((s) => ({ comments: { ...s.comments, [id]: (s.comments[id] ?? []).filter((_, i) => i !== index) } }))
  },

  requestChanges: async () => {
    const cur = get().current
    if (!cur) return
    const comments = get().comments[cur.plan.id] ?? []
    if (comments.length === 0) return
    set({ busy: true, error: null })
    try {
      // Revising is still planning.
      if (!get().planMode) await get().setPlanMode(true)
      await useAgentStore.getState().sendMessage(feedbackMessage(cur.plan, comments))
      set((s) => ({ comments: { ...s.comments, [cur.plan.id]: [] } }))
    } catch (e) {
      set({ error: String(e) })
    } finally {
      set({ busy: false })
    }
  },

  approve: async () => {
    const cur = get().current
    const r = root()
    if (!cur || !r) return
    set({ busy: true, error: null })
    try {
      const message = await call<string>('cmd_plan_approve', { threadId: '', root: r, id: cur.plan.id, version: cur.plan.version })
      set({ planMode: false })
      if (message) await useAgentStore.getState().sendMessage(message)
    } catch (e) {
      set({ error: String(e) })
    } finally {
      set({ busy: false })
    }
  },

  cancelExecution: async () => {
    const cur = get().current
    if (cur) await call('cmd_plan_cancel', { id: cur.plan.id })
  },
}))
