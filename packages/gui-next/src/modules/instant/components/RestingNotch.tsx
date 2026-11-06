import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Box, ButtonBase } from '../../../ui'
import { call, on } from '../../../ipc'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { RobotHeadState } from 'robot-heads'
import { NotchFace } from './NotchFace'
import { useInstantStore } from '../store/instantStore'
import { useCountUp, useStaggerIn, usePop, reducedMotion, EASE_OUT } from '../lib/motion'
import gsap from 'gsap'

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
  return <NotchFace state={faceOf(a)} size={size} name={a.kind === 'chat' ? 'Vivianne' : a.name} vivianne={a.kind === 'chat'} />
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

/** Mirrors the `cmd_get_usage_history` day rows. */
interface UsageDay {
  input: number
  output: number
  cost: number
  requests: number
  hasUnknownCost: boolean
}

/** Local `YYYY-MM-DD`, the key `agent::usage_store` files each day under. */
function dayKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function compact(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(Math.round(n))
}

interface Usage {
  /** Tokens per day, oldest first, ending today. */
  week: number[]
  today: UsageDay | null
}

function usageFrom(history: Record<string, UsageDay>): Usage {
  const week: number[] = []
  const d = new Date()
  d.setDate(d.getDate() - 6)
  for (let i = 0; i < 7; i++) {
    const day = history[dayKey(d)]
    week.push(day ? day.input + day.output : 0)
    d.setDate(d.getDate() + 1)
  }
  return { week, today: history[dayKey(new Date())] ?? null }
}

/** Today's tokens and spend, with the last seven days as bars. */
function UsageStrip({ usage }: { usage: Usage }) {
  const tokens = useCountUp(usage.today ? usage.today.input + usage.today.output : 0)
  const cost = useCountUp(usage.today?.cost ?? 0)
  const bars = useRef<HTMLDivElement | null>(null)
  const max = Math.max(1, ...usage.week)
  const weekKey = usage.week.join()
  useLayoutEffect(() => {
    if (!bars.current || reducedMotion()) return
    const t = gsap.from(bars.current.children, { scaleY: 0, duration: 0.6, ease: EASE_OUT, stagger: 0.04, transformOrigin: 'bottom', clearProps: 'transform' })
    return () => void t.revert()
  }, [weekKey])
  const unknown = usage.today?.hasUnknownCost
  return (
    <Box data-anim sx={{ display: 'flex', alignItems: 'flex-end', gap: 1.25, py: 0.75, mb: 0.5 }}>
      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Box sx={{ fontSize: 10.5, color: FAINT, textTransform: 'uppercase', letterSpacing: '0.06em', mb: 0.25 }}>Today</Box>
        <Box sx={{ display: 'flex', alignItems: 'baseline', gap: 0.75, fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
          <Box component="span" sx={{ fontSize: 15, fontWeight: 600 }}>
            {compact(tokens)}
          </Box>
          <Box component="span" sx={{ fontSize: 11, color: MUTED }}>
            tokens ·
          </Box>
          <Box
            component="span"
            sx={{ fontSize: 12, fontWeight: 600 }}
            title={unknown ? 'Some models have no known price, so the real total is at least this.' : undefined}
          >
            {unknown && !usage.today?.cost ? '—' : `$${cost.toFixed(2)}${unknown ? '+' : ''}`}
          </Box>
        </Box>
      </Box>
      <Box ref={bars} title="Tokens, last 7 days" sx={{ display: 'flex', alignItems: 'flex-end', gap: '3px', height: 24 }}>
        {usage.week.map((v, i) => (
          <Box
            key={i}
            sx={{
              width: 5,
              height: `${Math.max(8, (v / max) * 100)}%`,
              borderRadius: '2px',
              bgcolor: i === 6 ? INK : 'rgba(255,255,255,0.22)',
              opacity: v ? 1 : 0.5,
            }}
          />
        ))}
      </Box>
    </Box>
  )
}

/** The fields of `agent::automation::Automation` the notch uses. */
interface Automation {
  id: string
  name: string
  enabled: boolean
}

/** One-tap asks, run by Viv Instant right here in the notch. */
const QUICK: { label: string; ask: string }[] = [
  { label: 'Explain my screen', ask: 'Take a screenshot of my screen and tell me briefly what is on it and anything worth acting on.' },
  { label: 'Catch me up', ask: 'What are my agents doing?' },
]

/** The last part of a path, either separator. */
const nameOf = (p: string) => p.split(/[\\/]/).pop() || p

/** What to ask about files dropped on the notch. */
const DROP_ACTIONS: { label: string; ask: (files: string) => string }[] = [
  { label: 'Summarize', ask: (f) => `Summarize ${f} briefly.` },
  { label: 'Explain', ask: (f) => `Explain what ${f} is and what's worth knowing about it.` },
]

export function RestingNotch() {
  const [agents, setAgents] = useState<AgentStatus[]>([])
  // Files over the notch (still held) or dropped on it (waiting for a choice).
  const [drop, setDrop] = useState<{ held: boolean; paths: string[] } | null>(null)
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
    // Files dragged from the desktop or a file manager arrive as paths.
    try {
      offs.push(
        getCurrentWebview().onDragDropEvent((e) => {
          const p = e.payload
          if (p.type === 'enter') setDrop({ held: true, paths: p.paths })
          else if (p.type === 'drop') setDrop(p.paths.length ? { held: false, paths: p.paths } : null)
          else if (p.type === 'leave') setDrop((d) => (d?.held ? null : d))
        }),
      )
    } catch {
      // No webview (a plain browser tab): nothing to drop on.
    }
    return () => offs.forEach((p) => void p.then((off) => off()))
  }, [])

  // A drop nobody answers goes away on its own.
  useEffect(() => {
    if (!drop || drop.held) return
    const t = window.setTimeout(() => setDrop(null), 20_000)
    return () => window.clearTimeout(t)
  }, [drop])

  // Run something through Viv Instant and switch the notch to its working
  // view, so the run shows its steps and answer like a spoken one does.
  const runInstant = (instruction: string, said: string) => {
    const s = useInstantStore.getState()
    s.activate()
    s.setTranscript(said)
    s.setPhase('working')
    void call('cmd_instant_run', { instruction }).catch((e) => useInstantStore.getState().fail(e instanceof Error ? e.message : String(e)))
  }
  const askAbout = (paths: string[], ask: (files: string) => string) => {
    const files = paths.map((p) => `"${p}"`).join(', ')
    setDrop(null)
    runInstant(ask(files), ask(files))
  }
  const openInChat = (paths: string[]) => {
    setDrop(null)
    void call('cmd_notch_open_with_files', { paths }).catch(() => {})
  }

  const [usage, setUsage] = useState<Usage | null>(null)
  const [automations, setAutomations] = useState<Automation[]>([])
  // Fresh each time it opens: cheap local reads, and the numbers move.
  useEffect(() => {
    if (!open) return
    void call<Record<string, UsageDay>>('cmd_get_usage_history').then((h) => h && setUsage(usageFrom(h))).catch(() => {})
    void call<Automation[]>('cmd_automation_list').then((a) => a && setAutomations(a.filter((x) => x.enabled).slice(0, 3))).catch(() => {})
  }, [open])
  const [ran, setRan] = useState<string | null>(null)
  const runAutomation = (a: Automation) => {
    setRan(a.id)
    void call('cmd_automation_run_now', { id: a.id }).catch(() => {})
    window.setTimeout(() => setRan((r) => (r === a.id ? null : r)), 1600)
  }

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

  const panel = useRef<HTMLDivElement | null>(null)
  useStaggerIn(panel, `${open}:${!!drop}:${drop?.held}`)
  const leadFace = useRef<HTMLDivElement | null>(null)

  const needsYou = live.filter((a) => a.state === 'needs_you')
  const lead = needsYou[0] ?? live[0]
  const recent = agents.filter((a) => !live.includes(a))
  const earliest = live.reduce((m, a) => Math.min(m, a.startedAt), Number.POSITIVE_INFINITY)
  usePop(leadFace, lead ? `${lead.id}:${faceOf(lead)}` : null)

  return (
    <Box sx={{ display: 'flex', alignItems: 'flex-start', width: 'fit-content', lineHeight: 0, transform: prefs.scale === 1 ? undefined : `scale(${prefs.scale})`, transformOrigin: 'top left' }}>
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
          lineHeight: 'normal',
          width: open ? 360 : 'fit-content',
          transition: 'border-radius 240ms cubic-bezier(0.22, 1, 0.36, 1), width 240ms cubic-bezier(0.22, 1, 0.36, 1)',
        }}
      >
        {!open && !lead && !drop && <Box sx={{ width: 160, height: 10 }} />}

        {drop && (
          <Box ref={panel} sx={{ width: 320, px: 1.5, py: 1.25 }}>
            {drop.held ? (
              <Box data-anim sx={{ fontSize: 12, fontWeight: 600, textAlign: 'center', py: 0.5 }}>
                Drop {drop.paths.length === 1 ? nameOf(drop.paths[0]) : `${drop.paths.length} files`} to ask Vivianne
              </Box>
            ) : (
              <>
                <Box data-anim sx={{ fontSize: 12, fontWeight: 600, mb: 0.25 }}>
                  {drop.paths.length === 1 ? nameOf(drop.paths[0]) : `${drop.paths.length} files`}
                </Box>
                <Box data-anim sx={{ fontSize: 11, color: MUTED, mb: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {drop.paths.length === 1 ? drop.paths[0] : drop.paths.map(nameOf).join(', ')}
                </Box>
                <Box data-anim sx={{ display: 'flex', gap: 0.75, flexWrap: 'wrap' }}>
                  {DROP_ACTIONS.map((a) => (
                    <Pill key={a.label} onClick={() => askAbout(drop.paths, a.ask)}>
                      {a.label}
                    </Pill>
                  ))}
                  <Pill onClick={() => openInChat(drop.paths)}>Open in chat</Pill>
                  <Pill onClick={() => setDrop(null)}>Never mind</Pill>
                </Box>
              </>
            )}
          </Box>
        )}

        {!open && !drop && lead && (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, height: 26, px: 1.25, maxWidth: 380 }}>
            <Box ref={leadFace} sx={{ display: 'flex', flexShrink: 0 }}>
              <Face a={lead} size={18} />
            </Box>
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

        {open && !drop && (
          <Box ref={panel} sx={{ px: 1.5, pt: 1, pb: 1.25 }}>
            <Box data-anim sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
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
            {usage && <UsageStrip usage={usage} />}
            <Box data-anim sx={{ display: 'flex', gap: 0.75, flexWrap: 'wrap', pb: 0.75, mb: 0.25, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
              {QUICK.map((q) => (
                <Pill key={q.label} onClick={() => runInstant(q.ask, q.label)}>
                  {q.label}
                </Pill>
              ))}
              {automations.map((a) => (
                <Pill key={a.id} onClick={() => runAutomation(a)} title="Run this automation now">
                  {ran === a.id ? 'Started ✓' : `▸ ${a.name}`}
                </Pill>
              ))}
            </Box>
            {prefs.showAgents &&
              [...live, ...recent.slice(0, 3)].map((a) => (
                <Box data-anim key={a.id}>
                  <Row a={a} now={now} />
                </Box>
              ))}
            {!agents.length && (
              <Box data-anim sx={{ fontSize: 11.5, color: MUTED, py: 0.5 }}>
                Nothing running. Hold {chord} and ask anything, or tell an agent what to do.
              </Box>
            )}
          </Box>
        )}
      </Box>
    </Box>
  )
}
