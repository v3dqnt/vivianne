import { create } from 'zustand'
import { call } from '../../../ipc'

/** Mirrors `coding_agent::commands::PlanModeInfo` — `#[serde(rename_all = "camelCase")]`. */
interface PlanModeInfo {
  active: boolean
  plan: string | null
}

interface PlanModeStoreState extends PlanModeInfo {
  refresh: () => Promise<void>
}

/** Driven by `agentStore` calling `refresh()` whenever the `enter_plan_mode`/
 * `exit_plan_mode` tools fire — no poll loop needed, those are the only two
 * things that ever change plan-mode state. */
export const usePlanModeStore = create<PlanModeStoreState>((set) => ({
  active: false,
  plan: null,

  refresh: async () => {
    const info = await call<PlanModeInfo>('cmd_coding_plan_mode')
    if (info) set(info)
  },
}))
