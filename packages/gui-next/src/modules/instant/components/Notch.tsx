import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Box, ButtonBase, keyframes } from '../../../ui'
import { call } from '../../../ipc'
import type { RobotHeadState } from 'robot-heads'
import { NotchFace } from './NotchFace'
import { usePop, reducedMotion, EASE_OUT } from '../lib/motion'
import gsap from 'gsap'

/** What the notch is saying, at a glance. */
export type NotchTone = 'listening' | 'working' | 'done' | 'stopped' | 'error' | 'idle'

// The notch is hardware-black in both themes: it reads as part of the
// screen's edge, not as an app window, and it sits over arbitrary content.
const INK = '#f4f4f5'
const MUTED = 'rgba(244,244,245,0.58)'
const FAINT = 'rgba(244,244,245,0.4)'
const SURFACE = '#0b0b0c'

/** How long the pill takes to settle into a new size. */
const SETTLE_MS = 320
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'

const bounce = keyframes`
  0%, 100% { transform: scaleY(0.35); }
  50%      { transform: scaleY(1); }
`

/**
 * The wave: five bars. Listening, they follow the microphone; working, they
 * move on their own, so a pause between actions still looks alive.
 */
function Wave({ tone, level }: { tone: NotchTone; level: number }) {
  const color = tone === 'error' ? '#ff6b6b' : tone === 'done' ? '#4ade80' : INK
  const still = tone === 'done' || tone === 'stopped' || tone === 'error' || tone === 'idle'
  const shape = [0.45, 0.75, 1, 0.75, 0.45]
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: '2px', height: 14, width: 18, flexShrink: 0 }}>
      {shape.map((k, i) => (
        <Box
          key={i}
          sx={{
            width: 2,
            height: 14,
            borderRadius: 1,
            bgcolor: color,
            opacity: tone === 'stopped' ? 0.5 : 1,
            transformOrigin: 'center',
            transition: 'transform 140ms cubic-bezier(0.22, 1, 0.36, 1)',
            transform:
              tone === 'listening'
                ? `scaleY(${Math.max(0.18, Math.min(1, level * 9 * k))})`
                : still
                  ? `scaleY(${0.3 + k * 0.25})`
                  : undefined,
            animation: tone === 'working' ? `${bounce} 900ms ease-in-out ${i * 110}ms infinite` : undefined,
          }}
        />
      ))}
    </Box>
  )
}

function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(t)
  }, [])
  const s = Math.max(0, Math.floor((now - since) / 1000))
  const text = s >= 3600 ? `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  return (
    <Box component="span" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: 11, color: FAINT, flexShrink: 0 }}>
      {text}
    </Box>
  )
}

export interface NotchProps {
  tone: NotchTone
  /** The robot's face; defaults to one that follows `tone`. */
  face?: RobotHeadState
  /** Bold, short: "Vivianne", "Listening". */
  title: string
  /** One line of what's happening; clipped, never wraps. */
  detail?: string | null
  /** Show a running clock from this time (ms since epoch). */
  since?: number | null
  onStop?: (() => void) | null
  stopLabel?: string
  /** Microphone level 0..1 while listening. */
  level?: number
  /** A second line under the row, only when there's something to read. */
  body?: ReactNode
  /** A thin bar along the bottom edge, 0..1 (a countdown, a download). */
  progress?: number | null
  /** The window-resize command for this overlay (the notch hugs its content). */
  resizeCommand: string
  minWidth?: number
}

/**
 * The notch: a black pill flush with the top of the screen, one line tall.
 * Used by Viv Instant and by the "Vivianne is using your computer" activity
 * indicator, so both speak the same visual language.
 *
 * It sizes its own window to its content, so the transparent window never
 * covers more of the screen than the notch itself does.
 */
/** The face a tone shows when the caller doesn't pick one. */
function faceFor(tone: NotchTone): RobotHeadState {
  return tone === 'done' ? 'happy' : tone === 'stopped' ? 'sleeping' : tone
}

export function Notch({
  tone,
  face,
  title,
  detail,
  since,
  onStop,
  stopLabel = 'Stop',
  level = 0,
  body,
  progress,
  resizeCommand,
  minWidth = 200,
}: NotchProps) {
  // The content is measured in an inner box; the pill itself animates to that
  // size, and the window is kept at least as big as the pill throughout so
  // nothing is clipped mid-animation. Resizing the window directly would snap.
  const ref = useRef<HTMLDivElement | null>(null)
  const [size, setSize] = useState<{ w: number; h: number } | null>(null)
  const [animate, setAnimate] = useState(false)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    let sent = { w: 0, h: 0 }
    let timer = 0
    const push = (w: number, h: number) => {
      if (w === sent.w && h === sent.h) return
      sent = { w, h }
      void call(resizeCommand, { width: w, height: h }).catch(() => {})
    }
    const sync = () => {
      // +2 for the side borders.
      const w = Math.ceil(el.offsetWidth) + 2
      const h = Math.ceil(el.offsetHeight)
      setSize((prev) => (prev && prev.w === w && prev.h === h ? prev : { w, h }))
      // Grow the window first, shrink it once the pill has finished easing.
      window.clearTimeout(timer)
      push(Math.max(sent.w, w), Math.max(sent.h, h))
      timer = window.setTimeout(() => push(w, h), SETTLE_MS + 60)
    }
    sync()
    const ro = new ResizeObserver(sync)
    ro.observe(el)
    const enable = window.setTimeout(() => setAnimate(true), 50)
    return () => {
      ro.disconnect()
      window.clearTimeout(timer)
      window.clearTimeout(enable)
    }
  }, [resizeCommand])

  // The face pops when the state changes; the title and the second line
  // slide in when they change, so a new step reads as news.
  const faceRef = useRef<HTMLDivElement | null>(null)
  usePop(faceRef, tone)
  const titleRef = useRef<HTMLSpanElement | null>(null)
  const bodyRef = useRef<HTMLDivElement | null>(null)
  const hasBody = !!body
  useLayoutEffect(() => {
    if (!titleRef.current || reducedMotion()) return
    const t = gsap.from(titleRef.current, { y: 5, opacity: 0, duration: 0.35, ease: EASE_OUT, clearProps: 'all' })
    return () => void t.revert()
  }, [title])
  useLayoutEffect(() => {
    if (!bodyRef.current || reducedMotion()) return
    const t = gsap.from(bodyRef.current, { y: -6, opacity: 0, duration: 0.45, delay: 0.08, ease: EASE_OUT, clearProps: 'all' })
    return () => void t.revert()
  }, [hasBody])

  return (
    // A block flex row, not an inline box: an inline-flex wrapper sits on a
    // text baseline and its line box leaves a gap above the pill, which is
    // what kept the notch from touching the top of the screen.
    <Box sx={{ display: 'flex', justifyContent: 'center', alignItems: 'flex-start', lineHeight: 0 }}>
      <Box
        sx={{
          position: 'relative',
          width: size ? size.w : 'fit-content',
          height: size ? size.h : 'auto',
          minWidth,
          maxWidth: 462,
          bgcolor: SURFACE,
          color: INK,
          borderRadius: '0 0 16px 16px',
          border: '1px solid rgba(255,255,255,0.08)',
          borderTop: 0,
          boxSizing: 'border-box',
          overflow: 'hidden',
          fontFamily: 'inherit',
          lineHeight: 'normal',
          userSelect: 'none',
          transition: animate ? `width ${SETTLE_MS}ms ${EASE}, height ${SETTLE_MS}ms ${EASE}` : 'none',
        }}
      >
      <Box ref={ref} sx={{ width: 'max-content', minWidth: minWidth - 2, maxWidth: 460 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, height: 34, pl: 1.5, pr: onStop ? 0.75 : 1.5, minWidth: 0 }}>
          <Box ref={faceRef} sx={{ display: 'flex', flexShrink: 0 }}>
            <NotchFace state={face ?? faceFor(tone)} />
          </Box>
          {/* While listening, the bars still follow the microphone: proof it hears you. */}
          {tone === 'listening' && <Wave tone={tone} level={level} />}
          <Box ref={titleRef} component="span" sx={{ fontSize: 12, fontWeight: 600, letterSpacing: '-0.005em', flexShrink: 0 }}>
            {title}
          </Box>
          {detail && (
            <Box
              component="span"
              title={detail}
              sx={{ fontSize: 12, color: MUTED, minWidth: 0, flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 300 }}
            >
              {detail}
            </Box>
          )}
          {since ? <Elapsed since={since} /> : null}
          {onStop && (
            <ButtonBase
              onClick={onStop}
              aria-label={stopLabel}
              sx={{
                gap: 0.625,
                height: 22,
                px: 1,
                ml: 0.25,
                borderRadius: '999px',
                fontSize: 11,
                fontWeight: 600,
                color: INK,
                bgcolor: 'rgba(255,255,255,0.09)',
                flexShrink: 0,
                transition: 'background-color 200ms ease, color 200ms ease',
                '&:hover': { bgcolor: 'rgba(255,90,90,0.22)', color: '#ffb4b4' },
              }}
            >
              <Box sx={{ width: 7, height: 7, borderRadius: '1.5px', bgcolor: 'currentColor' }} />
              {stopLabel}
            </ButtonBase>
          )}
        </Box>
        {body && (
          <Box ref={bodyRef} sx={{ px: 1.5, pb: 1.25, mt: -0.25, fontSize: 12, lineHeight: 1.45, color: INK, maxWidth: 460 }}>
            {body}
          </Box>
        )}
      </Box>
        {progress != null && (
          <Box sx={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: 2, bgcolor: 'rgba(255,255,255,0.08)' }}>
            <Box sx={{ height: '100%', width: `${Math.round(Math.max(0, Math.min(1, progress)) * 100)}%`, bgcolor: INK, transition: 'width 160ms linear' }} />
          </Box>
        )}
      </Box>
    </Box>
  )
}

export const NOTCH_MUTED = MUTED
