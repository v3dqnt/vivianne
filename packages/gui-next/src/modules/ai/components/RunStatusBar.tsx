import { useEffect, useState } from 'react'
import { Box, Button, Stack, TextField, Typography } from '../../../ui'
import { useAgentStore } from '../store/agentStore'
import type { RunStatus } from '../lib/types'

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
  const [addUsd, setAddUsd] = useState('10')
  const [addHours, setAddHours] = useState('24')
  const [error, setError] = useState<string | null>(null)
  const status: RunStatus | null = ledger ? (waiting ? 'waiting' : ledger.status) : waiting ? 'waiting' : null
  const now = useNow(status === 'running' || status === 'waiting')


  if (!status || status === 'done' || status === 'stopped') return null
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
    meta.push(ledger.capUsd !== null ? `$${ledger.spentUsd.toFixed(2)} of $${ledger.capUsd.toFixed(2)}` : `$${ledger.spentUsd.toFixed(2)} spent`)
    meta.push(`${formatDuration(now - ledger.startedAt)} elapsed`)
    if (ledger.todos.length) meta.push(`${ledger.milestones.length} of ${ledger.todos.length} done`)
    else if (ledger.milestones.length) meta.push(`${ledger.milestones.length} checkpoints`)
    if (ledger.model) meta.push(ledger.model.split('/').pop() ?? ledger.model)
  }

  return (
    <Box
      sx={{
        border: 1,
        borderColor: 'divider',
        borderRadius: '12px',
        bgcolor: 'background.paper',
        px: 1.5,
        py: 1,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, minWidth: 0 }}>
        <Box
          sx={{
            width: 7,
            height: 7,
            borderRadius: '50%',
            bgcolor: color,
            flexShrink: 0,
            animation: status === 'running' ? 'vivPulse 1.6s ease-in-out infinite' : undefined,
            '@keyframes vivPulse': { '0%, 100%': { opacity: 1 }, '50%': { opacity: 0.35 } },
          }}
        />
        <Typography variant="caption" sx={{ fontWeight: 600, color, flexShrink: 0 }}>
          {label}
          {status === 'waiting' && waitUntil ? ` — resumes in ${formatDuration(waitUntil - now)}` : ''}
        </Typography>
        {ledger && (
          <Typography variant="caption" color="text.secondary" noWrap sx={{ minWidth: 0 }} title={ledger.objective}>
            {ledger.objective}
          </Typography>
        )}
        <Box sx={{ flex: 1 }} />
        {ledger && (status === 'running' || status === 'waiting') && (
          <Button size="small" color="inherit" onClick={() => void run(() => control('pause'))}>
            Pause
          </Button>
        )}
        {!ledger && status === 'waiting' && (
          <Button size="small" color="inherit" onClick={() => void abort()}>
            Stop
          </Button>
        )}
        {ledger && (status === 'paused' || status === 'blocked') && !capHit && (
          <Button size="small" variant="contained" onClick={() => void run(() => control('resume'))}>
            Resume
          </Button>
        )}
        {ledger && (
          <Button size="small" color="inherit" onClick={() => void run(() => control('stop'))}>
            End task
          </Button>
        )}
      </Stack>
      {(meta.length > 0 || (reason && status !== 'running')) && (
        <Typography variant="caption" color="text.disabled" sx={{ display: 'block', mt: 0.5, pl: 2 }}>
          {[...meta, status !== 'running' && reason ? reason : null].filter(Boolean).join(' · ')}
        </Typography>
      )}
      {ledger && capHit && (status === 'paused' || status === 'blocked') && (
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mt: 1, pl: 2 }}>
          <TextField label="Add budget ($)" size="small" value={addUsd} onChange={(e) => setAddUsd(e.target.value)} sx={{ width: 120 }} />
          <TextField label="Add time (h)" size="small" value={addHours} onChange={(e) => setAddHours(e.target.value)} sx={{ width: 120 }} />
          <Button
            size="small"
            variant="contained"
            onClick={() => void run(() => control('resume', { usd: numberOr(addUsd), hours: numberOr(addHours) }))}
          >
            Raise and resume
          </Button>
        </Stack>
      )}
      {error && (
        <Typography variant="caption" color="error.main" sx={{ display: 'block', mt: 0.5, pl: 2 }}>
          {error}
        </Typography>
      )}
    </Box>
  )
}
