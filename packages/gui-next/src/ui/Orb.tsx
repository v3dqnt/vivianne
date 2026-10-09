import { ThinkingOrb, type OrbState } from 'thinking-orbs'
import { useTheme } from './ThemeProvider'

export type { OrbState }

/**
 * The app's activity indicator: a dotted thought-orb (`thinking-orbs`) in
 * place of a pulsing dot, with a state per kind of activity — `working`,
 * `searching`, `solving`, `listening`, `connecting`, `composing`.
 *
 * The orb's own theme detection looks for a `data-theme` attribute or a
 * `dark` class, which this app doesn't set, so the palette's mode is passed
 * in. It renders at one of its tuned sizes (20, 32, 64); `px` scales that
 * drawing for spots smaller than 20px, like a badge on an avatar. It stops
 * animating for people who ask for reduced motion.
 */
export function Orb({
  state = 'working',
  size = 20,
  px,
  color,
  label,
}: {
  state?: OrbState
  size?: 20 | 32 | 64
  /** Displayed size in CSS px, when it differs from `size`. */
  px?: number
  /** Tint, as `#rrggbb` or `rgb()`; grayscale ink when omitted. */
  color?: string
  label?: string
}) {
  const theme = useTheme()
  const shown = px ?? size
  return (
    <ThinkingOrb
      state={state}
      size={size}
      theme={theme.palette.mode === 'dark' ? 'dark' : 'light'}
      color={color}
      aria-label={label ?? state}
      style={{ width: shown, height: shown, flexShrink: 0 }}
    />
  )
}
