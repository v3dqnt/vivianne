import { useEffect, useMemo, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Box from '@mui/material/Box'
import Tooltip from '@mui/material/Tooltip'
import ButtonBase from '@mui/material/ButtonBase'
import { call } from '../../../ipc'

export interface UsageDay {
  input: number
  output: number
  cost: number
  requests: number
  /** True if at least one request that day used a model with no known
   * price — `cost` is a real but partial total in that case, not the full
   * day's spend. See `ai::pricing` on the Rust side for the priced models. */
  hasUnknownCost: boolean
}

const DAY_MS = 24 * 60 * 60 * 1000
const WEEKS = 53
const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

type ViewMode = 'daily' | 'weekly' | 'cumulative'

function formatDate(d: Date): string {
  return d.toISOString().slice(0, 10)
}

/** 12.4M / 340K / 512 — the reference stat tiles read as magnitudes, and a
 * raw nine-digit token count is unreadable at a glance. */
function compact(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

interface Cell {
  date: string
  day: UsageDay | null
  /** Whatever the current view mode is colouring by. */
  value: number
}

/** Longest run of consecutive days with any usage, and the run ending today
 * (or yesterday — a streak shouldn't read as broken before the day is out). */
function streaks(history: Record<string, UsageDay>): { current: number; longest: number } {
  const active = new Set(Object.keys(history).filter((k) => history[k].requests > 0))
  if (active.size === 0) return { current: 0, longest: 0 }

  const sorted = [...active].sort()
  let longest = 1
  let run = 1
  for (let i = 1; i < sorted.length; i++) {
    const prev = new Date(`${sorted[i - 1]}T00:00:00Z`).getTime()
    const cur = new Date(`${sorted[i]}T00:00:00Z`).getTime()
    if (cur - prev === DAY_MS) {
      run += 1
      longest = Math.max(longest, run)
    } else {
      run = 1
    }
  }

  const today = new Date()
  today.setHours(0, 0, 0, 0)
  let cursor = today
  // Yesterday still counts: today may simply not have started yet.
  if (!active.has(formatDate(cursor))) cursor = new Date(today.getTime() - DAY_MS)
  let current = 0
  while (active.has(formatDate(cursor))) {
    current += 1
    cursor = new Date(cursor.getTime() - DAY_MS)
  }
  return { current, longest }
}

/** Usage dashboard — lifetime totals, streaks, and a year-long activity
 * heatmap, all from `cmd_get_usage_history` (real provider-reported token
 * counts recorded per day by `agent::usage_store`, not estimates). Every
 * figure here is derived from that one daily series; nothing is stubbed. */
export function UsageSection() {
  const [history, setHistory] = useState<Record<string, UsageDay>>({})
  const [loading, setLoading] = useState(true)
  const [mode, setMode] = useState<ViewMode>('daily')

  useEffect(() => {
    let active = true
    void call<Record<string, UsageDay>>('cmd_get_usage_history').then((h) => {
      if (active) {
        setHistory(h ?? {})
        setLoading(false)
      }
    })
    return () => {
      active = false
    }
  }, [])

  const { cells, maxValue, totals, months, peakDay, activeDays } = useMemo(() => {
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const days = WEEKS * 7
    const start = new Date(today.getTime() - (days - 1) * DAY_MS)
    // Back up to Sunday so every column is a whole week.
    start.setDate(start.getDate() - start.getDay())

    const list: Cell[] = []
    const sum = { input: 0, output: 0, cost: 0, requests: 0, hasUnknownCost: false }
    let peak = { date: '', tokens: 0 }
    let active = 0

    for (let i = 0; i < days; i++) {
      const d = new Date(start.getTime() + i * DAY_MS)
      const key = formatDate(d)
      const day = history[key] ?? null
      if (day) {
        const tokens = day.input + day.output
        sum.input += day.input
        sum.output += day.output
        sum.cost += day.cost
        sum.requests += day.requests
        sum.hasUnknownCost = sum.hasUnknownCost || day.hasUnknownCost
        if (day.requests > 0) active += 1
        if (tokens > peak.tokens) peak = { date: key, tokens }
      }
      list.push({ date: key, day, value: 0 })
    }

    // Fill `value` per view mode. Weekly gives every cell in a column its
    // column total, so the heatmap reads as week-over-week bands; cumulative
    // is the running lifetime total, which only ever climbs.
    if (mode === 'daily') {
      for (const c of list) c.value = c.day ? c.day.input + c.day.output : 0
    } else if (mode === 'weekly') {
      for (let col = 0; col * 7 < list.length; col++) {
        const week = list.slice(col * 7, col * 7 + 7)
        const total = week.reduce((n, c) => n + (c.day ? c.day.input + c.day.output : 0), 0)
        for (const c of week) c.value = total
      }
    } else {
      let running = 0
      for (const c of list) {
        running += c.day ? c.day.input + c.day.output : 0
        c.value = running
      }
    }

    const max = list.reduce((m, c) => Math.max(m, c.value), 0)

    // One label per month, positioned at the column where that month first
    // appears — matches the reference's Sep…Aug axis.
    const labels: { col: number; name: string }[] = []
    let lastMonth = -1
    for (let col = 0; col * 7 < list.length; col++) {
      const d = new Date(`${list[col * 7].date}T00:00:00Z`)
      const m = d.getUTCMonth()
      if (m !== lastMonth) {
        labels.push({ col, name: MONTH_NAMES[m] })
        lastMonth = m
      }
    }

    return { cells: list, maxValue: max, totals: sum, months: labels, peakDay: peak, activeDays: active }
  }, [history, mode])

  const { current: currentStreak, longest: longestStreak } = useMemo(() => streaks(history), [history])

  const lifetimeTokens = totals.input + totals.output

  // No priced request contributed anything real — showing "$0.00" here would
  // read as a confirmed zero rather than "unknown", so it shows as "—".
  const costLabel =
    totals.requests === 0
      ? '$0.00'
      : totals.cost === 0 && totals.hasUnknownCost
        ? '—'
        : `$${totals.cost.toFixed(2)}${totals.hasUnknownCost ? '+' : ''}`

  const stats = [
    { label: 'Lifetime tokens', value: compact(lifetimeTokens) },
    { label: 'Peak day', value: compact(peakDay.tokens) },
    { label: 'Active days', value: String(activeDays) },
    { label: 'Current streak', value: currentStreak === 1 ? '1 day' : `${currentStreak} days` },
    { label: 'Longest streak', value: longestStreak === 1 ? '1 day' : `${longestStreak} days` },
  ]

  const insights: { label: string; value: string }[] = [
    { label: 'Total requests', value: totals.requests.toLocaleString() },
    { label: 'Input tokens', value: totals.input.toLocaleString() },
    { label: 'Output tokens', value: totals.output.toLocaleString() },
    {
      label: 'Avg tokens / active day',
      value: activeDays > 0 ? compact(Math.round(lifetimeTokens / activeDays)) : '—',
    },
    { label: 'Busiest day', value: peakDay.date || '—' },
    { label: 'Estimated cost', value: costLabel },
  ]

  return (
    <Stack sx={{ gap: 2.5 }}>
      {/* Stat band — one row, evenly divided, like the reference's header. */}
      <Stack
        direction="row"
        sx={{
          flexWrap: 'wrap',
          borderRadius: 1.2,
          border: 1,
          borderColor: 'divider',
          overflow: 'hidden',
        }}
      >
        {stats.map((s, i) => (
          <Box
            key={s.label}
            sx={{
              flex: '1 1 140px',
              px: 2,
              py: 1.75,
              textAlign: 'center',
              borderLeft: i === 0 ? 0 : 1,
              borderColor: 'divider',
            }}
          >
            <Typography sx={{ fontSize: '1.375rem', fontWeight: 600, lineHeight: 1.2 }}>{s.value}</Typography>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
              {s.label}
            </Typography>
          </Box>
        ))}
      </Stack>

      {/* Activity heatmap */}
      <Box sx={{ p: 2, borderRadius: 1.2, border: 1, borderColor: 'divider' }}>
        <Stack direction="row" sx={{ alignItems: 'center', mb: 1.5, gap: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600, flex: 1 }}>
            Token activity
          </Typography>
          <Stack direction="row" sx={{ gap: 0.25 }}>
            {(['daily', 'weekly', 'cumulative'] as ViewMode[]).map((m) => (
              <ButtonBase
                key={m}
                onClick={() => setMode(m)}
                sx={{
                  px: 1,
                  py: 0.375,
                  borderRadius: 1.5,
                  fontSize: '0.71875rem',
                  textTransform: 'capitalize',
                  color: mode === m ? 'text.primary' : 'text.secondary',
                  bgcolor: mode === m ? 'action.selected' : 'transparent',
                  '&:hover': { bgcolor: mode === m ? 'action.selected' : 'action.hover' },
                }}
              >
                {m}
              </ButtonBase>
            ))}
          </Stack>
        </Stack>

        {loading ? (
          <Typography variant="caption" color="text.secondary">
            Loading…
          </Typography>
        ) : (
          <Box sx={{ overflowX: 'auto' }}>
            <Box sx={{ display: 'inline-block', minWidth: 0 }}>
              <Box
                sx={{
                  display: 'grid',
                  gridAutoFlow: 'column',
                  gap: '3px',
                  gridTemplateRows: 'repeat(7, 11px)',
                }}
              >
                {cells.map((cell) => {
                  const tokens = cell.day ? cell.day.input + cell.day.output : 0
                  return (
                    <Tooltip
                      key={cell.date}
                      disableInteractive
                      title={
                        cell.day
                          ? `${cell.date} — ${cell.day.requests} request${cell.day.requests === 1 ? '' : 's'}, ${tokens.toLocaleString()} tokens`
                          : `${cell.date} — no activity`
                      }
                    >
                      <Box
                        sx={{
                          width: 11,
                          height: 11,
                          borderRadius: '2px',
                          bgcolor: cell.value > 0 ? 'primary.main' : 'action.hover',
                          // Floor at 0.2 so a real-but-small day is still
                          // visibly distinct from an empty one.
                          opacity: cell.value > 0 ? Math.max(0.2, cell.value / (maxValue || 1)) : 1,
                        }}
                      />
                    </Tooltip>
                  )
                })}
              </Box>
              {/* Month axis — absolute-positioned per column so labels line
                  up with the week each month actually starts in. */}
              <Box sx={{ position: 'relative', height: 16, mt: 0.75 }}>
                {months.map((m) => (
                  <Typography
                    key={`${m.name}-${m.col}`}
                    variant="caption"
                    color="text.secondary"
                    sx={{ position: 'absolute', left: m.col * 14, fontSize: '0.6875rem' }}
                  >
                    {m.name}
                  </Typography>
                ))}
              </Box>
            </Box>
          </Box>
        )}

        {!loading && maxValue === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>
            No usage recorded yet — this fills in as you use real provider models.
          </Typography>
        )}
      </Box>

      {/* Insights */}
      <Box sx={{ p: 2, borderRadius: 1.2, border: 1, borderColor: 'divider' }}>
        <Typography variant="body2" sx={{ fontWeight: 600, mb: 1.5 }}>
          Activity insights
        </Typography>
        <Stack sx={{ gap: 0 }}>
          {insights.map((row, i) => (
            <Stack
              key={row.label}
              direction="row"
              sx={{
                alignItems: 'center',
                justifyContent: 'space-between',
                py: 1,
                borderTop: i === 0 ? 0 : 1,
                borderColor: 'divider',
                gap: 2,
              }}
            >
              <Typography variant="body2" color="text.secondary">
                {row.label}
              </Typography>
              <Typography variant="body2" sx={{ fontWeight: 500, fontFamily: '"JetBrains Mono", ui-monospace, monospace' }}>
                {row.value}
              </Typography>
            </Stack>
          ))}
        </Stack>
        {totals.hasUnknownCost && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>
            Cost is partial — some requests used a model without known pricing, so the real total is at least this much.
          </Typography>
        )}
      </Box>
    </Stack>
  )
}
