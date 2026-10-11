import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Box, ButtonBase, keyframes } from '../../../ui'
import { call } from '../../../ipc'
import type { RobotHeadState } from 'robot-heads'
import { NotchFace } from './NotchFace'

/** What the notch is saying, at a glance. */
export type NotchTone = 'listening' | 'working' | 'done' | 'stopped' | 'error' | 'idle'

// The notch is hardware-black in both themes: it reads as part of the
// screen's edge, not as an app window, and it sits over arbitrary content.
const INK = '#f4f4f5'
const MUTED = 'rgba(244,244,245,0.58)'
const FAINT = 'rgba(244,244,245,0.4)'
const SURFACE = '#0b0b0c'

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
            transition: 'transform 90ms linear',
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
  const ref = useRef<HTMLDivElement | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    let last = ''
    const sync = () => {
      const r = el.getBoundingClientRect()
      const width = Math.ceil(r.width)
      const height = Math.ceil(r.height)
      const key = `${width}x${height}`
      if (key === last) return
      last = key
      void call(resizeCommand, { width, height }).catch(() => {})
    }
    sync()
    const ro = new ResizeObserver(sync)
    ro.observe(el)
    return () => ro.disconnect()
  }, [resizeCommand])

  return (
    <Box sx={{ display: 'inline-flex' }}>
      <Box
        ref={ref}
        sx={{
          position: 'relative',
          width: 'fit-content',
          minWidth,
          maxWidth: 460,
          bgcolor: SURFACE,
          color: INK,
          borderRadius: '0 0 16px 16px',
          border: '1px solid rgba(255,255,255,0.08)',
          borderTop: 0,
          overflow: 'hidden',
          fontFamily: 'inherit',
          userSelect: 'none',
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, height: 34, pl: 1.5, pr: onStop ? 0.75 : 1.5, minWidth: 0 }}>
          <NotchFace state={face ?? faceFor(tone)} />
          {/* While listening, the bars still follow the microphone: proof it hears you. */}
          {tone === 'listening' && <Wave tone={tone} level={level} />}
          <Box component="span" sx={{ fontSize: 12, fontWeight: 600, letterSpacing: '-0.005em', flexShrink: 0 }}>
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
                transition: 'background-color 120ms, color 120ms',
                '&:hover': { bgcolor: 'rgba(255,90,90,0.22)', color: '#ffb4b4' },
              }}
            >
              <Box sx={{ width: 7, height: 7, borderRadius: '1.5px', bgcolor: 'currentColor' }} />
              {stopLabel}
            </ButtonBase>
          )}
        </Box>
        {body && (
          <Box sx={{ px: 1.5, pb: 1.25, mt: -0.25, fontSize: 12, lineHeight: 1.45, color: INK, maxWidth: 460 }}>
            {body}
          </Box>
        )}
        {progress != null && (
          <Box sx={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: 2, bgcolor: 'rgba(255,255,255,0.08)' }}>
            <Box sx={{ height: '100%', width: `${Math.round(Math.max(0, Math.min(1, progress)) * 100)}%`, bgcolor: INK, transition: 'width 120ms linear' }} />
          </Box>
        )}
      </Box>
    </Box>
  )
}

export const NOTCH_MUTED = MUTED
