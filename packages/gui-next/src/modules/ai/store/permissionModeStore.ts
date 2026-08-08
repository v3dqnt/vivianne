import { create } from 'zustand'
import { call } from '../../../ipc'

/**
 * "Auto" (bypass) vs "Ask for approval" — persisted the same way the old
 * frontend did it, as a `bypassMode` boolean inside the generic
 * `cmd_get_settings`/`cmd_save_settings` blob. The backend reads this same
 * flag the next time a gated tool call checks it, so toggling here actually
 * changes whether tool calls pause for approval, not just a label.
 */
interface PermissionModeState {
  bypassMode: boolean
  loaded: boolean
  refresh: () => Promise<void>
  toggle: () => Promise<void>
}

export const usePermissionModeStore = create<PermissionModeState>((set, get) => ({
  bypassMode: false,
  loaded: false,

  refresh: async () => {
    const settings = await call<{ bypassMode?: boolean }>('cmd_get_settings')
    set({ bypassMode: !!settings?.bypassMode, loaded: true })
  },

  toggle: async () => {
    const next = !get().bypassMode
    set({ bypassMode: next })
    const current = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
    await call('cmd_save_settings', { settings: { ...current, bypassMode: next } })
  },
}))
