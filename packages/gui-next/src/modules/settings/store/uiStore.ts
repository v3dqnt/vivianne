import { create } from 'zustand'

export type SettingsSection =
  | 'providers'
  | 'local'
  | 'mods'
  | 'subagents'
  | 'mcp'
  | 'memory'
  | 'usage'
  | 'skills'
  | 'automations'
  | 'themes'
  | 'components'

interface SettingsUiState {
  open: boolean
  section: SettingsSection
  openSettings: (section?: SettingsSection) => void
  closeSettings: () => void
}

/** Controls the Settings dialog's open/active-tab state — shared so any
 * component (sidebar button, "manage local models" link in the model
 * picker, ...) can open Settings to a specific tab without prop drilling. */
export const useSettingsUi = create<SettingsUiState>((set) => ({
  open: false,
  section: 'providers',
  openSettings: (section) => set((s) => ({ open: true, section: section ?? s.section })),
  closeSettings: () => set({ open: false }),
}))
