import type { Mode, Tokens } from './ui/tokens.ts'

/**
 * The single source of visual truth: Terax's design language (see
 * `ui/theme.ts`). The default theme is monochrome — "primary" is the ink
 * colour. Presets add a colour identity by swapping only `primary` and
 * `ring`; every neutral stays put, so the chrome reads the same in all of
 * them. Picked from Settings → Theme & layout.
 */

export type TokenSet = Omit<Tokens, 'frame'>

/** Terax's default neutrals. */
export const NEUTRAL: Record<Mode, TokenSet> = {
  light: {
    background: '#ffffff',
    foreground: '#111111',
    card: '#f3f3f3',
    popover: '#ffffff',
    primary: '#111111',
    primaryForeground: '#ffffff',
    secondary: '#ededed',
    muted: '#ededed',
    mutedForeground: '#656565',
    accent: '#e5e5e5',
    destructive: '#c0392f',
    border: 'rgba(0, 0, 0, 0.09)',
    input: 'rgba(0, 0, 0, 0.13)',
    ring: '#8a8a8a',
  },
  dark: {
    background: '#141414',
    foreground: '#f7f7f7',
    card: '#1d1d1d',
    popover: '#1d1d1d',
    primary: '#f7f7f7',
    primaryForeground: '#141414',
    secondary: '#2a2a2a',
    muted: '#1d1d1d',
    mutedForeground: '#a3a3a3',
    accent: '#2a2a2a',
    destructive: '#e35d5d',
    border: 'rgba(255, 255, 255, 0.09)',
    input: 'rgba(255, 255, 255, 0.13)',
    ring: '#8a8a8a',
  },
}

export interface PaletteDef {
  id: string
  label: string
  symbol: string
  swatches: string[]
  /** Per mode: the tokens this preset changes. Usually just the accent. */
  light: Partial<TokenSet>
  dark: Partial<TokenSet>
}

export const accent = (color: string, onColor: string) => ({ primary: color, primaryForeground: onColor, ring: color })

/**
 * The shared neutral system for Vivianne's own palettes: cool slate with a
 * 1-2% blue cast, three clear steps (frame < pane < raised), and three
 * text levels that each clear WCAG AA on every surface: primary (14:1),
 * secondary (6:1 and up) and tertiary (4.6:1, `text.disabled`). A palette on top of it only chooses the one accent, which is
 * spent on selection, focus and the primary action and nothing else.
 */
export const SLATE = {
  light: {
    background: '#fbfbfc',
    card: '#f1f2f4',
    popover: '#ffffff',
    foreground: '#14161a',
    mutedForeground: '#4f5560',
    secondary: '#e8eaee',
    muted: '#eef0f2',
    accent: '#e3e6ea',
    border: 'rgba(20, 22, 26, 0.11)',
    input: 'rgba(20, 22, 26, 0.18)',
  },
  dark: {
    background: '#1b1e23',
    card: '#22252b',
    popover: '#272a31',
    foreground: '#e9ebef',
    mutedForeground: '#a8aeb9',
    secondary: '#2d3037',
    muted: '#22252b',
    accent: '#2d3037',
    border: 'rgba(255, 255, 255, 0.10)',
    input: 'rgba(255, 255, 255, 0.16)',
  },
} satisfies Record<Mode, Partial<TokenSet>>

export const PALETTES: PaletteDef[] = [
  // Vivianne's own: silver and black, no hue. The accent is the metal itself: silver on the dark theme,
  // ink on the light one. Colour is kept for state only (done, needs you, failed), never for decoration.
  {
    id: 'silver',
    label: 'Silver',
    symbol: '◇',
    swatches: ['#d9dbde', '#0c0d0f', '#141518', '#eef0f2'],
    light: {
      background: '#e4e6e9',
      card: '#eceef0',
      popover: '#f4f5f6',
      foreground: '#0e0f11',
      mutedForeground: '#44474d',
      secondary: '#d6d8dc',
      muted: '#dcdee1',
      accent: '#d2d5d9',
      border: 'rgba(14, 15, 17, 0.12)',
      input: 'rgba(14, 15, 17, 0.2)',
      ...accent('#0e0f11', '#eef0f2'),
      ring: '#44474d',
    },
    dark: {
      background: '#0c0d0f',
      card: '#141518',
      popover: '#18191c',
      foreground: '#eef0f2',
      mutedForeground: '#a9acb1',
      secondary: '#202226',
      muted: '#141518',
      accent: '#202226',
      border: 'rgba(255, 255, 255, 0.09)',
      input: 'rgba(255, 255, 255, 0.14)',
      ...accent('#d9dbde', '#0e0f11'),
      ring: '#a9acb1',
    },
  },
  {
    id: 'aero',
    label: 'Aero',
    symbol: '◌',
    swatches: ['#3bb4e6', '#14161a', '#262930', '#e9ebef'],
    light: { ...SLATE.light, ...accent('#0a7099', '#ffffff') },
    dark: { ...SLATE.dark, ...accent('#3bb4e6', '#06141c') },
  },
  {
    id: 'moss',
    label: 'Moss',
    symbol: '❖',
    swatches: ['#3ecf8e', '#14161a', '#262930', '#e9ebef'],
    light: { ...SLATE.light, ...accent('#126b47', '#ffffff') },
    dark: { ...SLATE.dark, ...accent('#3ecf8e', '#06170f') },
  },
  {
    id: 'rose',
    label: 'Rose',
    symbol: '✿',
    swatches: ['#f2678a', '#14161a', '#262930', '#e9ebef'],
    light: { ...SLATE.light, ...accent('#b82a53', '#ffffff') },
    dark: { ...SLATE.dark, ...accent('#f57a98', '#1c0810') },
  },
  {
    id: 'nvidia',
    label: 'NVIDIA',
    symbol: '▲',
    swatches: ['#76b900', '#0a0a0a', '#1f1f1f', '#f5f5f5'],
    light: {
      foreground: '#111111',
      mutedForeground: '#4a4f44',
      ...accent('#436c00', '#ffffff'),
    },
    dark: {
      background: '#0a0a0a',
      card: '#121212',
      popover: '#121212',
      secondary: '#1c1c1c',
      muted: '#121212',
      accent: '#1f2a10',
      border: 'rgba(255, 255, 255, 0.08)',
      ...accent('#76b900', '#0a0a0a'),
    },
  },
  {
    id: 'nemotron',
    label: 'Nemotron',
    symbol: '◆',
    swatches: ['#76b900', '#0b140a', '#17250f', '#e9f1dc'],
    light: {
      background: '#f7faf1',
      card: '#edf3e0',
      popover: '#f7faf1',
      foreground: '#121a0b',
      mutedForeground: '#5b6a4a',
      secondary: '#e3ebd2',
      muted: '#e3ebd2',
      accent: '#dbe6c4',
      border: 'rgba(18, 26, 11, 0.10)',
      ...accent('#477300', '#ffffff'),
    },
    dark: {
      background: '#0b140a',
      card: '#111d0d',
      popover: '#111d0d',
      foreground: '#e9f1dc',
      mutedForeground: '#9db086',
      secondary: '#182713',
      muted: '#111d0d',
      accent: '#1c2f14',
      border: 'rgba(118, 185, 0, 0.16)',
      input: 'rgba(118, 185, 0, 0.22)',
      ...accent('#76b900', '#0b140a'),
    },
  },
  {
    id: 'terax',
    label: 'Monochrome',
    symbol: '◐',
    swatches: ['#f7f7f7', '#a3a3a3', '#2a2a2a', '#141414'],
    light: {},
    dark: {},
  },
  {
    id: 'vivianne',
    label: 'Vivianne',
    symbol: '☀',
    swatches: ['#4a9eff', '#1a73e8', '#1d1d1d', '#f7f7f7'],
    light: accent('#1768d1', '#ffffff'),
    dark: accent('#4a9eff', '#0a0a0a'),
  },
  {
    id: 'claude',
    label: 'Claude',
    symbol: '✳',
    swatches: ['#d97757', '#262624', '#b0a89a', '#f5f4ee'],
    light: {
      background: '#faf9f5',
      card: '#f0eee6',
      popover: '#faf9f5',
      foreground: '#1f1e1d',
      mutedForeground: '#5e5a53',
      secondary: '#e8e6dc',
      muted: '#e8e6dc',
      accent: '#e3e0d4',
      border: 'rgba(31, 30, 29, 0.10)',
      ...accent('#9d4d32', '#ffffff'),
    },
    dark: {
      background: '#1f1e1d',
      card: '#262624',
      popover: '#262624',
      foreground: '#f5f4ee',
      mutedForeground: '#b0a89a',
      secondary: '#30302e',
      muted: '#262624',
      accent: '#30302e',
      border: 'rgba(245, 244, 238, 0.10)',
      input: 'rgba(245, 244, 238, 0.13)',
      ...accent('#dd8568', '#1f1e1d'),
    },
  },
  {
    id: 'indigo',
    label: 'Space Indigo',
    symbol: '✦',
    swatches: ['#ef233c', '#2b2d42', '#8d99ae', '#edf2f4'],
    light: accent('#c41e30', '#ffffff'),
    dark: accent('#f5697b', '#0a0a0c'),
  },
  {
    id: 'graphite',
    label: 'Graphite',
    symbol: '◈',
    swatches: ['#a9a29c', '#5a5550', '#28262b', '#e8e4e0'],
    light: accent('#5a5550', '#ffffff'),
    dark: accent('#a9a29c', '#141414'),
  },
]
