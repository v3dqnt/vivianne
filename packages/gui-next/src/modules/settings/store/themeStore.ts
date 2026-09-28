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
/** The monochrome Terax palette became the default in the move to the
 * Terax design language. Anyone still on the old default ('vivianne', never
 * picked deliberately by most) is moved once; a later pick sticks. */
const MIGRATED_KEY = 'viv.theme.terax'
function initial(): { mode: ThemeMode; paletteId: string } {
  const saved = loadPersisted<{ mode: ThemeMode; paletteId: string }>(STORAGE_KEY, { mode: 'dark', paletteId: 'terax' })
  if (!loadPersisted<boolean>(MIGRATED_KEY, false)) {
    savePersisted(MIGRATED_KEY, true)
    if (saved.paletteId === 'vivianne') {
      saved.paletteId = 'terax'
      savePersisted(STORAGE_KEY, saved)
    }
  }
  return saved
}

export const useThemeStore = create<ThemeStoreState>((set) => ({
  ...initial(),
  setMode: (mode) => {
    savePersisted(STORAGE_KEY, { mode, paletteId: useThemeStore.getState().paletteId })
    set({ mode })
  },
  setPalette: (paletteId) => {
    savePersisted(STORAGE_KEY, { mode: useThemeStore.getState().mode, paletteId })
    set({ paletteId })
  },
}))
