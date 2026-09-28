import { Box, keyframes } from '../../../ui'
import type { InstantPhase } from '../store/instantStore'

const pulse = keyframes`
  0%   { transform: scale(1);    opacity: 0.55; }
  50%  { transform: scale(1.12); opacity: 0.9; }
  100% { transform: scale(1);    opacity: 0.55; }
`

const spin = keyframes`
  from { transform: rotate(0deg); }
  to   { transform: rotate(360deg); }
`

/** Fixed at each size, so dropping the real character in later cannot reflow
 * the text beside it.
 *
 * Two sizes, not a free-form prop: the notch row is only ~30px tall, and the
 * expanded card has room for a real stage. Allowing arbitrary sizes would make
 * the eventual 3D model responsible for looking right at any dimension, which
 * is exactly the coupling this seam exists to avoid. */
const SIZE = 132
const COMPACT_SIZE = 22

/**
 * Reserved stage for the Viv Instant character.
 *
 * The 3D model lands here later. Until then this draws a simple orb that
 * reacts to `phase`, so the overlay has a focal point and the layout is already
 * the shape it will keep.
 *
 * Three rules keep this seam clean, and they are the reason it takes a bare
 * `phase` rather than reading the store:
 *
 * - It receives **only** `phase` — never the store, never IPC. The character is
 *   decoration; nothing in the pipeline may come to depend on it.
 * - It is **fixed size**, so swapping in a canvas cannot move anything.
 * - When it becomes a `lazy()` three.js component it goes inside a `Suspense`
 *   and an error boundary, so a failed chunk degrades to an empty box rather
 *   than taking the overlay down with it.
 */
export function InstantCharacterSlot({
  phase,
  compact = false,
}: {
  phase: InstantPhase
  compact?: boolean
}) {
  const tone =
    phase === 'error' ? 'error.main' : phase === 'done' ? 'success.main' : 'primary.main'
  const animation =
    phase === 'listening'
      ? `${pulse} 1.6s ease-in-out infinite`
      : phase === 'working' || phase === 'transcribing'
        ? `${spin} 2.4s linear infinite`
        : 'none'

  const box = compact ? COMPACT_SIZE : SIZE
  const orb = compact ? COMPACT_SIZE : 84

  return (
    <Box
      sx={{
        width: box,
        height: box,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <Box
        sx={{
          width: orb,
          height: orb,
          borderRadius: '50%',
          bgcolor: tone,
          opacity: 0.55,
          filter: 'blur(1px)',
          boxShadow: (t) => `0 0 ${compact ? 10 : 40}px ${compact ? 2 : 6}px ${t.palette.primary.main}55`,
          animation,
        }}
      />
    </Box>
  )
}
