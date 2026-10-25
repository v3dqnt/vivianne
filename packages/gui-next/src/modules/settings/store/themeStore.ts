import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { call, hasTauri } from '../../../ipc'
import type { FontChoice, PaletteDef } from '../../../theme'

export type ThemeMode = 'light' | 'dark'
export type ModePref = ThemeMode | 'system'
export type Density = 'compact' | 'comfortable' | 'roomy'
export type ChatWidth = 'narrow' | 'normal' | 'wide' | 'full'

/** Everything about how the app looks, as the user chose it. */
export interface Appearance {
  modePref: ModePref
  paletteId: string
  /** Any colour over the palette's own accent; `null` keeps the palette's. */
  accent: string | null
  font: FontChoice
  /** Whole-app scale, 0.9–1.25. */
  uiScale: number
  /** Corner radius base in px (Terax: 10). */
  radius: number
  density: Density
  chatWidth: ChatWidth
  /** With a wallpaper: how much of it shows through the panes, 0–1. */
  glass: number
  /** Darkening over the wallpaper, 0–0.8. */
  wallpaperDim: number
}

export const DEFAULT_APPEARANCE: Appearance = {
  modePref: 'dark',
  paletteId: 'nvidia',
  accent: null,
  font: 'geist',
  uiScale: 1,
  radius: 10,
  density: 'comfortable',
  chatWidth: 'normal',
  glass: 0.35,
  wallpaperDim: 0.35,
}

interface ThemeStoreState extends Appearance {
  /** The mode in effect (`modePref`, or the OS's when following it). */
  mode: ThemeMode
  /** Themes the user made (`~/.viv/themes`). */
  customThemes: PaletteDef[]
  /** The wallpaper as a data URL, when there is one. */
  wallpaper: string | null
  /** While the theme builder previews a theme: painted instead of the saved one. */
  preview: PaletteDef | null
  setMode: (mode: ModePref) => void
  setPalette: (id: string) => void
  update: (patch: Partial<Appearance>) => void
  reset: () => void
  loadCustom: () => Promise<void>
  saveTheme: (theme: PaletteDef) => Promise<void>
  deleteTheme: (id: string) => Promise<void>
  setPreview: (theme: PaletteDef | null) => void
  loadWallpaper: () => Promise<void>
  setWallpaper: (path: string | null) => Promise<void>
}

const STORAGE_KEY = 'viv.appearance'
const OLD_KEY = 'viv.theme'
/** The monochrome Terax palette became the default in the move to the
 * Terax design language. Anyone still on the old default ('vivianne', never
 * picked deliberately by most) is moved once; a later pick sticks. */
const MIGRATED_KEY = 'viv.theme.terax'

const systemDark = () => typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches

function resolve(pref: ModePref): ThemeMode {
  if (pref === 'system') return systemDark() ? 'dark' : 'light'
  return pref
}

/** The NVIDIA edition opens in its own theme once, whatever was saved before;
 * a later pick sticks. */
const EDITION_THEMED_KEY = 'viv.hackathon.themed'

function initial(): Appearance {
  const saved = loadPersisted<Partial<Appearance> | null>(STORAGE_KEY, null)
  const base = (() => {
    if (saved) return { ...DEFAULT_APPEARANCE, ...saved }
    // From the old theme-only store; a fresh install takes the default palette.
    const old = loadPersisted<{ mode: ThemeMode; paletteId: string } | null>(OLD_KEY, null)
    if (!loadPersisted<boolean>(MIGRATED_KEY, false)) savePersisted(MIGRATED_KEY, true)
    if (!old) return { ...DEFAULT_APPEARANCE }
    return { ...DEFAULT_APPEARANCE, modePref: old.mode, paletteId: old.paletteId === 'vivianne' ? DEFAULT_APPEARANCE.paletteId : old.paletteId }
  })()
  if (!loadPersisted<boolean>(EDITION_THEMED_KEY, false)) {
    savePersisted(EDITION_THEMED_KEY, true)
    const themed = { ...base, paletteId: DEFAULT_APPEARANCE.paletteId }
    // Saved now, so it is the user's setting from here on, not re-derived.
    persist(themed)
    return themed
  }
  return base
}

function persist(s: Appearance) {
  const out: Appearance = {
    modePref: s.modePref,
    paletteId: s.paletteId,
    accent: s.accent,
    font: s.font,
    uiScale: s.uiScale,
    radius: s.radius,
    density: s.density,
    chatWidth: s.chatWidth,
    glass: s.glass,
    wallpaperDim: s.wallpaperDim,
  }
  savePersisted(STORAGE_KEY, out)
}

const start = initial()

/** The app's appearance: palette, mode, accent, type, size, density,
 * wallpaper — persisted locally (a desktop preference needs no backend), with
 * custom themes and the wallpaper image kept as files by the backend. */
export const useThemeStore = create<ThemeStoreState>((set, get) => ({
  ...start,
  mode: resolve(start.modePref),
  customThemes: [],
  wallpaper: null,
  preview: null,

  setMode: (modePref) => get().update({ modePref }),
  setPalette: (paletteId) => get().update({ paletteId }),

  update: (patch) => {
    set((s) => {
      const next = { ...s, ...patch }
      persist(next)
      return { ...patch, mode: resolve(next.modePref) }
    })
  },

  reset: () => {
    persist(DEFAULT_APPEARANCE)
    set({ ...DEFAULT_APPEARANCE, mode: resolve(DEFAULT_APPEARANCE.modePref) })
  },

  loadCustom: async () => {
    if (!hasTauri()) return
    const themes = (await call<PaletteDef[]>('cmd_themes_list')) ?? []
    set({ customThemes: themes })
  },

  saveTheme: async (theme) => {
    await call('cmd_themes_save', { theme })
    await get().loadCustom()
  },

  deleteTheme: async (id) => {
    await call('cmd_themes_delete', { id })
    if (get().paletteId === id) get().update({ paletteId: 'terax' })
    await get().loadCustom()
  },

  setPreview: (preview) => set({ preview }),

  loadWallpaper: async () => {
    if (!hasTauri()) return
    set({ wallpaper: (await call<string | null>('cmd_appearance_wallpaper')) ?? null })
  },

  setWallpaper: async (path) => {
    set({ wallpaper: (await call<string | null>('cmd_appearance_set_wallpaper', { path })) ?? null })
  },
}))

// Follow the OS when asked to.
if (typeof window !== 'undefined' && window.matchMedia) {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    const s = useThemeStore.getState()
    if (s.modePref === 'system') useThemeStore.setState({ mode: resolve('system') })
  })
}

/** The theme options the stores imply, for `buildTheme`/`themeTokens`. */
export function themeOptions(s: ThemeStoreState) {
  return {
    custom: s.preview ? [...s.customThemes.filter((t) => t.id !== s.preview!.id), s.preview] : s.customThemes,
    accent: s.accent,
    font: s.font,
    radius: s.radius,
    wallpaper: !!s.wallpaper,
    glass: s.glass,
  }
}

/** The palette id being painted (the previewed one while building). */
export const activePaletteId = (s: ThemeStoreState) => s.preview?.id ?? s.paletteId
