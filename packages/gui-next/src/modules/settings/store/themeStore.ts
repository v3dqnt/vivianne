import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

export type ThemeMode = 'light' | 'dark'

interface ThemeStoreState {
  mode: ThemeMode
  paletteId: string
  setMode: (mode: ThemeMode) => void
  setPalette: (id: string) => void
}

const STORAGE_KEY = 'viv.theme'

/** The visual theme — palette + light/dark — persisted locally so it survives
 * restarts without a backend round-trip (the old app pushed this into its
 * settings.json; a desktop-app preference is fine in localStorage). */
export const useThemeStore = create<ThemeStoreState>((set) => ({
  ...loadPersisted<{ mode: ThemeMode; paletteId: string }>(STORAGE_KEY, { mode: 'dark', paletteId: 'vivianne' }),
  setMode: (mode) => {
    savePersisted(STORAGE_KEY, { mode, paletteId: useThemeStore.getState().paletteId })
    set({ mode })
  },
  setPalette: (paletteId) => {
    savePersisted(STORAGE_KEY, { mode: useThemeStore.getState().mode, paletteId })
    set({ paletteId })
  },
}))
