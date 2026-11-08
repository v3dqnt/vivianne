import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Box, useTheme } from '../ui'

const HOLD_MS = 2000
const SWAP_MS = 150
const GAP_MS = 50
const DISTANCE = 8
const BLUR = 2
const SHIMMER_MS = 2000
const EASE = 'ease-in-out'

/**
 * A line of status text that swaps through `states`: the outgoing line exits up through a small blur while the next rises in from below
 * (the Thinking-states pattern from Transitions.dev), with a soft highlight sweeping the letters. In the app's own colors and type: it
 * inherits the surrounding font, muted text for the letters and primary text for the sweep.
 *
 * A hidden copy of the longest state holds the box's width, so nothing around it shifts mid-swap. With one state it just shimmers.
 * `hold` is how long each state stays before the next arrives.
 */
export function ThinkingStates({ states, hold = HOLD_MS }: { states: readonly string[]; hold?: number }) {
  const theme = useTheme()
  const [current, setCurrent] = useState(0)
  const [leaving, setLeaving] = useState<number | null>(null)
  const [entering, setEntering] = useState(false)
  const live = useRef<HTMLSpanElement | null>(null)
  const count = states.length

  // Hold, then swap to the next state.
  useEffect(() => {
    if (count < 2) return
    const t = setTimeout(() => {
      setLeaving(current)
      setCurrent((i) => (i + 1) % count)
      setEntering(true)
    }, hold)
    return () => clearTimeout(t)
  }, [current, count, hold])

  // Put the incoming line at its start pose before paint, release it a moment later, and drop the outgoing copy once its swap has played.
  useLayoutEffect(() => {
    if (!entering) return
    if (live.current) void live.current.offsetWidth
    const release = setTimeout(() => setEntering(false), GAP_MS)
    const done = setTimeout(() => setLeaving(null), SWAP_MS + GAP_MS)
    return () => {
      clearTimeout(release)
      clearTimeout(done)
    }
  }, [entering])

  if (count === 0) return null
  const longest = states.reduce((a, b) => (b.length > a.length ? b : a), '')
  const base = theme.palette.text.secondary
  const highlight = theme.palette.text.primary

  const text = {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    display: 'block',
    color: base,
    whiteSpace: 'nowrap',
    transform: 'translateY(0)',
    filter: 'blur(0)',
    opacity: 1,
    transition: `transform ${SWAP_MS}ms ${EASE}, filter ${SWAP_MS}ms ${EASE}, opacity ${SWAP_MS}ms ${EASE}`,
    willChange: 'transform, filter, opacity',
    // The sweep runs over the letters only.
    '&::before': {
      content: 'attr(data-text)',
      position: 'absolute',
      inset: 0,
      pointerEvents: 'none',
      backgroundImage: `linear-gradient(90deg, transparent 0%, transparent 40%, ${highlight} 50%, transparent 60%, transparent 100%)`,
      backgroundSize: '400% 100%',
      backgroundRepeat: 'no-repeat',
      WebkitBackgroundClip: 'text',
      backgroundClip: 'text',
      color: 'transparent',
      WebkitTextFillColor: 'transparent',
      animation: `viv-think-shimmer ${SHIMMER_MS}ms linear infinite`,
    },
    '&.is-exit': { transform: `translateY(${-DISTANCE}px)`, filter: `blur(${BLUR}px)`, opacity: 0 },
    '&.is-enter-start': { transition: 'none', transform: `translateY(${DISTANCE}px)`, filter: `blur(${BLUR}px)`, opacity: 0 },
    '@media (prefers-reduced-motion: reduce)': {
      transition: 'none',
      transform: 'none !important',
      filter: 'none !important',
      '&::before': { display: 'none' },
    },
  } as const

  return (
    <Box
      component="span"
      role="status"
      sx={{
        position: 'relative',
        display: 'inline-block',
        '@keyframes viv-think-shimmer': { from: { backgroundPosition: '100% 0' }, to: { backgroundPosition: '0% 0' } },
        '& .t-text': text,
      }}
    >
      <Box component="span" aria-hidden sx={{ display: 'block', visibility: 'hidden', whiteSpace: 'nowrap' }}>
        {longest}
      </Box>
      {leaving !== null && (
        <span className="t-text is-exit" data-text={states[leaving]} aria-hidden>
          {states[leaving]}
        </span>
      )}
      <span key={current} ref={live} className={`t-text${entering ? ' is-enter-start' : ''}`} data-text={states[current]}>
        {states[current]}
      </span>
    </Box>
  )
}
