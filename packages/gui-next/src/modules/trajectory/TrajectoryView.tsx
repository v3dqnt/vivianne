import { useEffect, useMemo, useState } from 'react'
import { Box, ButtonBase, IconButton, InputBase, Stack, Typography, alpha, useTheme } from '../../ui'
import { CloseLinear, SearchNormalLinear } from '../../icons'
import { call } from '../../ipc'
import { useAppView } from '../git/components/GitView'
import { buildTrajectory, type Row, type RowKind, type TraceEvent, type Turn } from './trajectoryModel'

interface TraceInfo {
  path: string
  startedMs: number
  source: string
  bytes: number
}

type Mode = 'duration' | 'turns' | 'calls'
const MODES: { id: Mode; label: string }[] = [
  { id: 'duration', label: 'Duration' },
  { id: 'turns', label: 'Turns' },
  { id: 'calls', label: 'Calls' },
]

const MONO = '"Geist Mono", ui-monospace, monospace'
const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}s` : `${Math.round(n)}ms`)
const clock = (t: number) => new Date(t).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit', second: '2-digit' })

const GLYPH: Record<RowKind, string> = { system: 'S', user: 'U', reminder: 'i', assistant: 'A', call: '›', result: '‹', compaction: '≋', notice: '·', end: '■' }

/** Debug view of one run: what was sent, how long each phase took, every call and its result. */
export function TrajectoryView() {
  const theme = useTheme()
  const [runs, setRuns] = useState<TraceInfo[] | null>(null)
  const [picked, setPicked] = useState<string | null>(null)
  const [events, setEvents] = useState<TraceEvent[]>([])
  const [error, setError] = useState<string | null>(null)
  const [mode, setMode] = useState<Mode>('duration')
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState<number | null>(null)

  useEffect(() => {
    void call<TraceInfo[]>('cmd_traces_list').then((r) => {
      setRuns(r ?? [])
      setPicked((p) => p ?? r?.[0]?.path ?? null)
    })
  }, [])
  useEffect(() => {
    if (!picked) return
    setError(null)
    setOpen(null)
    call<TraceEvent[]>('cmd_trace_read', { path: picked })
      .then((e) => setEvents(e ?? []))
      .catch((e) => setError(String(e instanceof Error ? e.message : e)))
  }, [picked])

  const traj = useMemo(() => buildTrajectory(events), [events])
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return traj.rows.filter((r) => (mode !== 'calls' || r.kind === 'call' || r.kind === 'result') && (!q || `${r.title} ${r.summary}`.toLowerCase().includes(q)))
  }, [traj, mode, query])
  const detail = open === null ? null : traj.rows.find((r) => r.id === open) ?? null

  return (
    <Stack sx={{ flex: 1, minHeight: 0, bgcolor: 'background.default' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', px: '16px', height: 44, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <ButtonBase onClick={() => useAppView.setState({ view: 'chat' })} sx={{ color: 'text.secondary', fontSize: '0.8125rem', '&:hover': { color: 'text.primary' } }}>← Back</ButtonBase>
        <Typography sx={{ fontWeight: 500 }}>Trajectory</Typography>
        <Box component="select" value={picked ?? ''} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setPicked(e.target.value)} sx={{ maxWidth: 420, bgcolor: 'action.hover', color: 'text.primary', border: 0, borderRadius: '6px', height: 28, px: '8px', fontSize: '0.75rem' }}>
          {(runs ?? []).map((r) => (
            <option key={r.path} value={r.path}>{`${new Date(r.startedMs).toLocaleString()} · ${r.source} · ${(r.bytes / 1024).toFixed(0)} KB`}</option>
          ))}
        </Box>
        <Box sx={{ flex: 1 }} />
        {MODES.map((m) => (
          <ButtonBase key={m.id} onClick={() => setMode(m.id)} sx={{ px: '10px', height: 28, borderRadius: '6px', fontSize: '0.8125rem', color: mode === m.id ? 'text.primary' : 'text.secondary', bgcolor: mode === m.id ? 'action.selected' : 'transparent', '&:hover': { color: 'text.primary' } }}>{m.label}</ButtonBase>
        ))}
        <Stack direction="row" sx={{ alignItems: 'center', gap: '6px', px: '10px', height: 28, borderRadius: '8px', bgcolor: 'action.hover', color: 'text.secondary', width: 220 }}>
          <SearchNormalLinear size={13} color="currentColor" />
          <InputBase value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search" sx={{ flex: 1, fontSize: '0.8125rem', color: 'text.primary' }} />
        </Stack>
      </Stack>

      {runs && runs.length === 0 && <Empty>No runs recorded yet. Run a task and it shows up here.</Empty>}
      {error && <Empty>{error}</Empty>}

      {traj.turns.length > 0 && !traj.turns.some((t) => t.modelMs > 0) && <Typography sx={{ px: '16px', py: '8px', fontSize: '0.75rem', color: 'text.disabled', borderBottom: 1, borderColor: 'divider' }}>This run was recorded before timings were kept, so there is no timeline. New runs have one.</Typography>}
      {traj.turns.some((t) => t.modelMs > 0) && <Timeline turns={traj.turns} start={traj.start} end={traj.end} onPick={(step) => setOpen(traj.rows.find((r) => r.step === step && r.kind === 'assistant')?.id ?? null)} />}

      <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
        <Box sx={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
          {shown.map((r) => (
            <RowLine key={r.id} row={r} active={r.id === open} first={traj.start} onOpen={() => setOpen(r.id === open ? null : r.id)} />
          ))}
          {shown.length === 0 && traj.rows.length > 0 && <Empty>Nothing matches.</Empty>}
        </Box>
        {detail && <Detail row={detail} onClose={() => setOpen(null)} accent={theme.palette.text.primary} />}
      </Stack>
    </Stack>
  )
}

function Empty({ children }: { children: string }) {
  return <Typography sx={{ p: '24px', color: 'text.secondary', fontSize: '0.875rem' }}>{children}</Typography>
}

/** Three lanes along the run's time: building the request (Input), the model, and the tools. */
function Timeline({ turns, start, end, onPick }: { turns: Turn[]; start: number; end: number; onPick: (step: number) => void }) {
  const theme = useTheme()
  const span = Math.max(1, end - start)
  const lanes = [
    { label: 'Input', color: alpha(theme.palette.text.primary, 0.55), at: (t: Turn) => [t.inputStart, t.inputMs] as const },
    { label: 'Model', color: theme.palette.text.primary, at: (t: Turn) => [t.modelStart, t.modelMs] as const },
    { label: 'Tools', color: theme.palette.success.main, at: (t: Turn) => [t.toolsStart, t.toolsMs] as const },
  ]
  return (
    <Box sx={{ flexShrink: 0, px: '16px', py: '8px', borderBottom: 1, borderColor: 'divider' }}>
      {lanes.map((lane) => (
        <Stack key={lane.label} direction="row" sx={{ alignItems: 'center', height: 20 }}>
          <Typography sx={{ width: 48, fontSize: '0.75rem', color: 'text.disabled' }}>{lane.label}</Typography>
          <Box sx={{ position: 'relative', flex: 1, height: 10 }}>
            {turns.map((t) => {
              const [from, len] = lane.at(t)
              if (len <= 0) return null
              return (
                <Box
                  key={t.step}
                  onClick={() => onPick(t.step)}
                  title={`Turn ${t.step} · ${lane.label} ${ms(len)}`}
                  sx={{ position: 'absolute', top: 0, height: 10, left: `${((from - start) / span) * 100}%`, width: `max(2px, ${(len / span) * 100}%)`, bgcolor: lane.color, borderRadius: '2px', cursor: 'pointer', '&:hover': { opacity: 0.75 } }}
                />
              )
            })}
          </Box>
        </Stack>
      ))}
      <Stack direction="row" sx={{ justifyContent: 'space-between', pl: '48px', fontSize: '0.75rem', color: 'text.disabled', fontFamily: MONO }}>
        <span>{clock(start)}</span>
        <span>{ms(span)}</span>
      </Stack>
    </Box>
  )
}

function RowLine({ row, active, first, onOpen }: { row: Row; active: boolean; first: number; onOpen: () => void }) {
  return (
    <ButtonBase
      onClick={onOpen}
      sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', alignItems: 'center', gap: '12px', px: '16px', height: 36, borderBottom: 1, borderColor: 'divider', textAlign: 'left', bgcolor: active ? 'action.selected' : 'transparent', '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover' } }}
    >
      <Box sx={{ width: 22, height: 24, flexShrink: 0, borderRadius: '6px', display: 'grid', placeItems: 'center', fontSize: '0.75rem', fontFamily: MONO, bgcolor: 'action.hover', color: row.failed ? 'error.main' : 'text.secondary' }}>{GLYPH[row.kind]}</Box>
      <Typography noWrap sx={{ flexShrink: 0, maxWidth: 260, fontSize: '0.8125rem', fontWeight: row.kind === 'user' || row.kind === 'assistant' ? 500 : 400, color: row.failed ? 'error.main' : 'text.primary', fontFamily: row.kind === 'call' ? MONO : 'inherit' }}>{row.title}</Typography>
      <Typography noWrap sx={{ flex: 1, minWidth: 0, fontSize: '0.8125rem', color: 'text.secondary' }}>{row.summary}</Typography>
      <Typography sx={{ flexShrink: 0, fontSize: '0.75rem', color: 'text.disabled', fontFamily: MONO }}>+{ms(row.t - first)}</Typography>
    </ButtonBase>
  )
}

/** `full`: no height cap, so nothing is behind an inner scrollbar (the system prompt must be readable in one piece). */
function Pretty({ label, value, full }: { label: string; value: unknown; full?: boolean }) {
  if (value === undefined || value === null || value === '') return null
  const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  return (
    <Box sx={{ mb: '12px' }}>
      <Typography sx={{ fontSize: '0.75rem', fontWeight: 500, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'text.disabled', mb: '4px' }}>{label}</Typography>
      <Box component="pre" sx={{ m: 0, p: '10px', borderRadius: '8px', bgcolor: 'action.hover', fontFamily: MONO, fontSize: '0.75rem', whiteSpace: 'pre-wrap', wordBreak: 'break-word', ...(full ? {} : { maxHeight: 360, overflow: 'auto' }) }}>{text}</Box>
    </Box>
  )
}

function Detail({ row, onClose }: { row: Row; onClose: () => void; accent: string }) {
  const [tab, setTab] = useState<'prompt' | 'tools'>('prompt')
  const e = row.event
  return (
    <Box sx={{ width: 'min(46%, 560px)', flexShrink: 0, borderLeft: 1, borderColor: 'divider', overflowY: 'auto', p: '16px' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', mb: '12px' }}>
        <Typography sx={{ flex: 1, fontWeight: 500 }}>{row.title}</Typography>
        <IconButton size="small" onClick={onClose} aria-label="Close"><CloseLinear size={14} /></IconButton>
      </Stack>
      {row.kind === 'system' ? (
        <>
          <Stack direction="row" sx={{ gap: '4px', mb: '12px' }}>
            {(['prompt', 'tools'] as const).filter((t) => (t === 'prompt' ? row.prompt : row.tools)).map((t) => (
              <ButtonBase key={t} onClick={() => setTab(t)} sx={{ px: '10px', height: 28, borderRadius: '6px', fontSize: '0.8125rem', color: tab === t ? 'text.primary' : 'text.secondary', bgcolor: tab === t ? 'action.selected' : 'transparent' }}>{t === 'prompt' ? 'System Prompt' : `Tools (${row.tools?.length ?? 0})`}</ButtonBase>
            ))}
          </Stack>
          {(tab === 'prompt' && row.prompt) || !row.tools ? (
            <Pretty label="System prompt" value={row.prompt} full />
          ) : (
            row.tools.map((t) => (
              <Box key={t.name} sx={{ py: '6px', borderBottom: 1, borderColor: 'divider' }}>
                <Typography sx={{ fontFamily: MONO, fontSize: '0.8125rem' }}>{t.name}</Typography>
                <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{t.description}</Typography>
              </Box>
            ))
          )}
        </>
      ) : (
        <>
          <Pretty label="Arguments" value={e.args} />
          <Pretty label="Result" value={e.result ?? e.text} />
          {row.kind === 'assistant' && (
            <Pretty
              label="Turn"
              value={{ model: e.model, answeredBy: e.answered_by, fallback: e.fallback, stop: e.stop_reason, inputTokens: e.input_tokens, outputTokens: e.output_tokens, inputMs: e.input_ms, modelMs: e.model_ms, contextMessages: e.context_messages, requestBytes: e.request_bytes, toolsOffered: e.tools_offered, error: e.error }}
            />
          )}
          <Pretty label="Event" value={Object.fromEntries(Object.entries(e).filter(([k]) => !['args', 'result', 'new_messages', 'system_prompt', 'tools', 'text'].includes(k)))} />
        </>
      )}
    </Box>
  )
}
