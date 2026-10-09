import { useEffect, useState } from 'react'
import { Box, Button, ButtonBase, Orb, Stack, TextField, Typography } from '../../../ui'
import { useAgentStore } from '../store/agentStore'
import type { RunStatus } from '../lib/types'

/** "12.4k", "1.2M". */
export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

/** "2h 13m", "4m 05s", "3d 2h" — durations at the precision that matters. */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000))
  const d = Math.floor(s / 86_400)
  const h = Math.floor((s % 86_400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = s % 60
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${String(sec).padStart(2, '0')}s`
  return `${sec}s`
}

const STATUS: Record<RunStatus, { label: string; color: string }> = {
  running: { label: 'Working', color: 'success.main' },
  waiting: { label: 'Waiting', color: 'warning.main' },
  paused: { label: 'Paused', color: 'text.secondary' },
  blocked: { label: 'Needs you', color: 'warning.main' },
  done: { label: 'Done', color: 'success.main' },
  stopped: { label: 'Stopped', color: 'text.secondary' },
}

/** Re-renders every second while `on`, for live countdowns. */
function useNow(on: boolean): number {
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    if (!on) return
    const t = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(t)
  }, [on])
  return now
}

/**
 * What a task that runs for a while is doing, at a glance: working, waiting out a
 * rate limit (with a countdown), paused on a cap, blocked, or done — with
 * its spend against budget, elapsed time and progress, and the controls to
 * pause, resume with more budget, or stop.
 *
 * Also covers a plain chat that is waiting out a rate limit, which is the
 * one moment an ordinary run otherwise looks frozen.
 */
export function RunStatusBar() {
  const ledger = useAgentStore((s) => s.longTask)
  const waiting = useAgentStore((s) => s.runWaiting)
  const control = useAgentStore((s) => s.controlLongTask)
  const abort = useAgentStore((s) => s.abort)
  const isBusy = useAgentStore((s) => s.isBusy)
  const stopping = useAgentStore((s) => s.stopping)
  const [addUsd, setAddUsd] = useState('10')
  const [addHours, setAddHours] = useState('24')
  const [error, setError] = useState<string | null>(null)
  const status: RunStatus | null = ledger ? (waiting ? 'waiting' : ledger.status) : waiting ? 'waiting' : null
  const now = useNow(status === 'running' || status === 'waiting')


  if (!status || status === 'done' || status === 'stopped') return null
  // "Working" is the run's to say: a ledger still reading running after the
  // run ended (or while it winds down from a stop) is stale, not a second
  // task to offer controls for.
  if ((status === 'running' || status === 'waiting') && (!isBusy || stopping)) return null
  // Every task has a ledger, and a quick exchange doesn't need a status bar:
  // a running task earns one once it is clearly a piece of work — it has a
  // plan, has taken a while, or is spending against a limit.
  if (
    status === 'running' &&
    ledger &&
    ledger.capUsd === null &&
    ledger.deadlineMs === null &&
    ledger.todos.length === 0 &&
    ledger.steps < 8
  ) {
    return null
  }

  const waitUntil = waiting?.untilMs ?? ledger?.waitUntil ?? null
  const reason = waiting?.reason ?? ledger?.statusReason ?? null
  const { label, color } = STATUS[status]
  const capHit = !!reason && /budget|time limit/.test(reason)
  const run = async (fn: () => Promise<void>) => {
    setError(null)
    try {
      await fn()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }
  const numberOr = (raw: string) => {
    const n = Number(raw)
    return Number.isFinite(n) && n > 0 ? n : undefined
  }

  const meta: string[] = []
  if (ledger) {
    const tokens = (ledger.inputTokens ?? 0) + (ledger.outputTokens ?? 0)
    if (ledger.spentUsd > 0 || ledger.capUsd !== null) {
      meta.push(ledger.capUsd !== null ? `$${ledger.spentUsd.toFixed(2)} of $${ledger.capUsd.toFixed(2)}` : `$${ledger.spentUsd.toFixed(2)}`)
    }
    if (tokens > 0) meta.push(`${formatTokens(tokens)} tokens`)
    meta.push(formatDuration(now - ledger.startedAt))
    if (ledger.todos.length) meta.push(`${ledger.milestones.length}/${ledger.todos.length} done`)
  }
  const note = status !== 'running' && reason ? reason : null

  const textButton = (label: string, onClick: () => void, strong = false) => (
    <ButtonBase
      onClick={onClick}
      sx={{
        px: 0.75,
        py: 0.25,
        borderRadius: '6px',
        fontSize: '0.6875rem',
        fontWeight: 600,
        color: strong ? 'text.primary' : 'text.secondary',
        flexShrink: 0,
        '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
      }}
    >
      {label}
    </ButtonBase>
  )

  return (
    // A tab on the composer's top edge: the same surface and hairline, open
    // at the bottom so it reads as part of the box it sits on, and inset so
    // the composer's own corners stay visible.
    <Box
      sx={{
        mx: 2,
        mb: '-1px',
        position: 'relative',
        zIndex: 1,
        border: 1,
        borderBottom: 0,
        borderColor: 'divider',
        borderRadius: '12px 12px 0 0',
        bgcolor: 'background.card',
        px: 1.25,
        py: 0.5,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0, minHeight: 24 }}>
        {status === 'running' || status === 'waiting' ? (
          <Orb state={status === 'running' ? 'working' : 'listening'} label={label} />
        ) : (
          <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: color, flexShrink: 0, mx: '7px' }} />
        )}
        <Typography variant="caption" sx={{ fontWeight: 600, color, flexShrink: 0, fontSize: '0.6875rem' }}>
          {label}
          {status === 'waiting' && waitUntil ? ` · ${formatDuration(waitUntil - now)}` : ''}
        </Typography>
        <Typography variant="caption" color="text.secondary" noWrap sx={{ minWidth: 0, flex: 1, fontSize: '0.6875rem' }} title={note ?? ledger?.objective}>
          {note ?? ledger?.objective ?? ''}
        </Typography>
        {meta.length > 0 && (
          <Typography className="cq-label" variant="caption" color="text.disabled" noWrap sx={{ flexShrink: 0, fontSize: '0.6875rem' }}>
            {meta.join(' · ')}
          </Typography>
        )}
        {ledger && (status === 'running' || status === 'waiting') && textButton('Pause', () => void run(() => control('pause')))}
        {!ledger && status === 'waiting' && textButton('Stop', () => void abort())}
        {ledger && (status === 'paused' || status === 'blocked') && !capHit && textButton('Resume', () => void run(() => control('resume')), true)}
        {ledger && textButton('End', () => void abort())}
      </Stack>
      {ledger && capHit && (status === 'paused' || status === 'blocked') && (
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center', py: 0.75 }}>
          <TextField label="Add budget ($)" size="small" value={addUsd} onChange={(e) => setAddUsd(e.target.value)} sx={{ width: 120 }} />
          <TextField label="Add time (h)" size="small" value={addHours} onChange={(e) => setAddHours(e.target.value)} sx={{ width: 120 }} />
          <Button size="small" variant="contained" onClick={() => void run(() => control('resume', { usd: numberOr(addUsd), hours: numberOr(addHours) }))}>
            Raise and resume
          </Button>
        </Stack>
      )}
      {error && (
        <Typography variant="caption" color="error.main" sx={{ display: 'block', pb: 0.5 }}>
          {error}
        </Typography>
      )}
    </Box>
  )
}
