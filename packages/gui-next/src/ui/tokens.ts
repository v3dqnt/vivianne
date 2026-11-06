// Leaf types, imported by both the app and the build config (which can't follow the rest of the UI kit).
export type Mode = 'light' | 'dark'

export interface Tokens {
  background: string
  foreground: string
  card: string
  popover: string
  primary: string
  primaryForeground: string
  secondary: string
  muted: string
  mutedForeground: string
  accent: string
  destructive: string
  border: string
  input: string
  ring: string
  /** The window background around the panes: one tone off `card`. */
  frame: string
}
