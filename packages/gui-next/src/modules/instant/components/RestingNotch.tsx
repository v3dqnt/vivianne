import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Box, ButtonBase } from '../../../ui'
import { call, on } from '../../../ipc'
import type { RobotHeadState } from 'robot-heads'
import { NotchFace } from './NotchFace'

/** Mirrors `notch::AgentStatus`. */
export interface AgentStatus {
  id: string
  kind: 'chat' | 'specialist' | 'subagent'
  name: string
  project: string | null
  task: string
  activity: string | null
  state: 'working' | 'needs_you' | 'done' | 'failed' | 'stopped'
  startedAt: number
  updatedAt: number
  thread: string | null
}

// Same hardware black as the Viv Instant notch (`Notch.tsx`).
const INK = '#f4f4f5'
const MUTED = 'rgba(244,244,245,0.58)'
const FAINT = 'rgba(244,244,245,0.38)'
const SURFACE = '#0b0b0c'
const AMBER = '#fbbf24'

function since(ms: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ms) / 1000))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

/** What an agent's face shows, from its status and what it's doing now. */
function faceOf(a: AgentStatus): RobotHeadState {
  if (a.state === 'needs_you') return 'listening'
  if (a.state === 'done') return 'happy'
  if (a.state === 'failed') return 'error'
  if (a.state === 'stopped') return 'sleeping'
  const act = a.activity ?? ''
  if (!act || act === 'Thinking') return 'thinking'
  if (/search|read|look|brows|fetch|grep|find|open|scan|recall/i.test(act)) return 'searching'
  return 'working'
}

/** An agent as a small robot head, in its own outline, showing its state. */
function Face({ a, size }: { a: AgentStatus; size: number }) {
  return <NotchFace state={faceOf(a)} size={size} name={a.kind === 'chat' ? 'Vivianne' : a.name} />
}

function Pill({ children, onClick, title }: { children: React.ReactNode; onClick: () => void; title?: string }) {
  return (
    <ButtonBase
      onClick={onClick}
      title={title}
      sx={{
        height: 22,
        px: 1,
        borderRadius: '999px',
        fontSize: 11,
        fontWeight: 600,
        color: INK,
        bgcolor: 'rgba(255,255,255,0.09)',
        flexShrink: 0,
        transition: 'background-color 120ms',
        '&:hover': { bgcolor: 'rgba(255,255,255,0.16)' },
      }}
    >
      {children}
    </ButtonBase>
  )
}

function Row({ a, now }: { a: AgentStatus; now: number }) {
  const who = a.kind === 'chat' ? (a.project ? `${a.project} lead` : a.name) : a.project ? `${a.name} · ${a.project}` : a.name
  const live = a.state === 'working' || a.state === 'needs_you'
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, py: 0.625, pl: a.kind === 'subagent' ? 2 : 0, minWidth: 0 }}>
      <Face a={a} size={20} />
      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Box sx={{ display: 'flex', gap: 0.75, alignItems: 'baseline', minWidth: 0 }}>
          <Box component="span" sx={{ fontSize: 12, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {who}
          </Box>
          <Box component="span" sx={{ fontSize: 11, color: a.state === 'needs_you' ? AMBER : FAINT, whiteSpace: 'nowrap', flexShrink: 0 }}>
            {a.state === 'needs_you' ? 'needs you' : live ? since(a.startedAt, now) : a.state}
          </Box>
        </Box>
        <Box
          title={a.task}
          sx={{ fontSize: 11.5, color: MUTED, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}
        >
          {live && a.activity && a.activity !== 'Thinking' ? a.activity : a.task}
        </Box>
      </Box>
      {a.thread !== null && live && (
        <Pill onClick={() => void call('cmd_notch_stop', { thread: a.thread })} title="Stop this agent">
          Stop
        </Pill>
      )}
    </Box>
  )
}

/**
 * The notch at rest: always at the top of the screen, saying what the agents
 * are doing. A thin tab when nothing is; a one-line summary when something
 * is; the whole list, with Stop for each, on hover.
 *
 * Like the Viv Instant notch, it sizes its window to itself, so it never
 * covers more of the screen than it shows.
 */
/** Mirrors `notch::NotchPrefs`. */
export interface NotchPrefs {
  position: 'center' | 'left' | 'right'
  scale: number
  showAgents: boolean
  showActivity: boolean
  showMic: boolean
}

export const DEFAULT_PREFS: NotchPrefs = { position: 'center', scale: 1, showAgents: true, showActivity: true, showMic: true }

export function RestingNotch() {
  const [agents, setAgents] = useState<AgentStatus[]>([])
  const [open, setOpen] = useState(false)
  const [now, setNow] = useState(Date.now())
  const [chord, setChord] = useState('the wake keys')
  const [prefs, setPrefs] = useState<NotchPrefs>(DEFAULT_PREFS)
  const ref = useRef<HTMLDivElement | null>(null)
  const closeTimer = useRef(0)

  useEffect(() => {
    void call<AgentStatus[]>('cmd_notch_status').then((a) => a && setAgents(a)).catch(() => {})
    void call<string>('cmd_instant_shortcut_label').then((l) => l && setChord(l)).catch(() => {})
    void call<NotchPrefs>('cmd_notch_prefs').then((p) => p && setPrefs(p)).catch(() => {})
    const offs: Array<Promise<() => void>> = [on<AgentStatus[]>('notch:status', setAgents), on<NotchPrefs>('notch:prefs', setPrefs)]
    return () => offs.forEach((p) => void p.then((off) => off()))
  }, [])

  const live = agents.filter((a) => a.state === 'working' || a.state === 'needs_you')
  useEffect(() => {
    if (!live.length && !open) return
    const t = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(t)
  }, [live.length, open])

  // The window hugs the notch. Measured after every render (the content
  // changes with every status), and by a ResizeObserver for anything that
  // resizes without one (fonts loading); unchanged sizes aren't re-sent.
  const lastSize = useRef('')
  const sync = useRef(() => {})
  sync.current = () => {
    const el = ref.current
    if (!el) return
    // The size preference is a transform, which the bounding box includes:
    // this is the drawn size.
    const r = el.getBoundingClientRect()
    const width = Math.ceil(r.width)
    const height = Math.ceil(r.height)
    const key = `${width}x${height}`
    if (key === lastSize.current) return
    lastSize.current = key
    void call('cmd_instant_set_size', { width, height }).catch(() => {})
  }
  useLayoutEffect(() => sync.current())
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(() => sync.current())
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // A short grace period on leave, so crossing the gap between the tab and
  // its own edge doesn't snap it shut.
  const enter = () => {
    window.clearTimeout(closeTimer.current)
    setOpen(true)
  }
  const leave = () => {
    closeTimer.current = window.setTimeout(() => setOpen(false), 250)
  }

  const needsYou = live.filter((a) => a.state === 'needs_you')
  const lead = needsYou[0] ?? live[0]
  const recent = agents.filter((a) => !live.includes(a))
  const earliest = live.reduce((m, a) => Math.min(m, a.startedAt), Number.POSITIVE_INFINITY)

  return (
    <Box sx={{ display: 'inline-flex', transform: prefs.scale === 1 ? undefined : `scale(${prefs.scale})`, transformOrigin: 'top left' }}>
      <Box
        ref={ref}
        onMouseEnter={enter}
        onMouseLeave={leave}
        sx={{
          bgcolor: SURFACE,
          color: INK,
          borderRadius: open ? '0 0 16px 16px' : '0 0 12px 12px',
          border: '1px solid rgba(255,255,255,0.08)',
          borderTop: 0,
          overflow: 'hidden',
          userSelect: 'none',
          width: open ? 360 : 'fit-content',
          transition: 'border-radius 120ms',
        }}
      >
        {!open && !lead && <Box sx={{ width: 120, height: 8 }} />}

        {!open && lead && (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, height: 26, px: 1.25, maxWidth: 380 }}>
            <Face a={lead} size={18} />
            <Box component="span" sx={{ fontSize: 11.5, fontWeight: 600, flexShrink: 0 }}>
              {needsYou.length ? 'Needs you' : live.length === 1 ? (lead.project ?? lead.name) : `${live.length} agents`}
            </Box>
            {prefs.showActivity && (
              <Box component="span" sx={{ fontSize: 11.5, color: MUTED, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', minWidth: 0 }}>
                {lead.activity && lead.activity !== 'Thinking' ? lead.activity : lead.task}
              </Box>
            )}
            {Number.isFinite(earliest) && (
              <Box component="span" sx={{ fontSize: 11, color: FAINT, fontFamily: '"Geist Mono", ui-monospace, monospace', flexShrink: 0 }}>
                {since(earliest, now)}
              </Box>
            )}
          </Box>
        )}

        {open && (
          <Box sx={{ px: 1.5, pt: 1, pb: 1.25 }}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
              <Box component="span" sx={{ fontSize: 12, fontWeight: 600, flex: 1 }}>
                {live.length ? `${live.length} working` : 'Vivianne'}
              </Box>
              {prefs.showMic && (
                <Pill onClick={() => void call('cmd_notch_talk')} title={`Or hold ${chord}`}>
                  Talk
                </Pill>
              )}
              <Pill onClick={() => void call('cmd_notch_open_main')}>Open</Pill>
            </Box>
            {prefs.showAgents &&
              live.map((a) => (
                <Row key={a.id} a={a} now={now} />
              ))}
            {prefs.showAgents &&
              recent.slice(0, 3).map((a) => (
                <Row key={a.id} a={a} now={now} />
              ))}
            {!agents.length && (
              <Box sx={{ fontSize: 11.5, color: MUTED, py: 0.5 }}>
                Nothing running. Hold {chord} and ask anything, or tell an agent what to do.
              </Box>
            )}
          </Box>
        )}
      </Box>
    </Box>
  )
}
