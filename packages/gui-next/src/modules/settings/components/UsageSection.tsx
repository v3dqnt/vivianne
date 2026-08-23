import { useEffect, useMemo, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Box from '@mui/material/Box'
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
const WEEKS = 26

function formatDate(d: Date): string {
  return d.toISOString().slice(0, 10)
}

/** Usage history heatmap (26 weeks) + totals, straight from the backend's
 * `cmd_get_usage_history` — the old frontend's identical panel, re-skinned. */
export function UsageSection() {
  const [history, setHistory] = useState<Record<string, UsageDay>>({})
  const [loading, setLoading] = useState(true)
  const [hovered, setHovered] = useState<{ date: string; day: UsageDay } | null>(null)

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

  const { cells, maxTotal, totals } = useMemo(() => {
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const days = WEEKS * 7
    const start = new Date(today.getTime() - (days - 1) * DAY_MS)
    start.setDate(start.getDate() - start.getDay())

    const list: { date: string; day: UsageDay | null }[] = []
    let max = 0
    const sum = { input: 0, output: 0, cost: 0, requests: 0, hasUnknownCost: false }

    for (let i = 0; i < days; i++) {
      const d = new Date(start.getTime() + i * DAY_MS)
      const key = formatDate(d)
      const day = history[key] ?? null
      if (day) {
        const total = day.input + day.output
        max = Math.max(max, total)
        sum.input += day.input
        sum.output += day.output
        sum.cost += day.cost
        sum.requests += day.requests
        sum.hasUnknownCost = sum.hasUnknownCost || day.hasUnknownCost
      }
      list.push({ date: key, day })
    }
    return { cells: list, maxTotal: max, totals: sum }
  }, [history])

  // No priced request contributed anything real — showing "$0.00" here would
  // read as a confirmed zero rather than "unknown," so it's shown as "—"
  // instead. A day with SOME known and some unknown-priced requests still
  // shows the known partial total, marked with "+" (real money, just not
  // the full amount).
  const costLabel =
    totals.requests === 0
      ? '$0.00'
      : totals.cost === 0 && totals.hasUnknownCost
        ? '—'
        : `$${totals.cost.toFixed(2)}${totals.hasUnknownCost ? '+' : ''}`

  return (
    <Stack sx={{ gap: 2 }}>
      <Stack direction="row" sx={{ gap: 1.5, flexWrap: 'wrap' }}>
        {[
          { label: 'Requests', value: totals.requests.toLocaleString() },
          { label: 'Input tokens', value: totals.input.toLocaleString() },
          { label: 'Output tokens', value: totals.output.toLocaleString() },
          { label: 'Est. cost', value: costLabel },
        ].map((stat) => (
          <Box key={stat.label} sx={{ flex: 1, minWidth: 140, p: 1.5, borderRadius: 2, border: 1, borderColor: 'divider' }}>
            <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.625rem' }}>
              {stat.label}
            </Typography>
            <Typography variant="body2" sx={{ fontWeight: 600, fontFamily: '"JetBrains Mono", ui-monospace, monospace', mt: 0.25 }}>
              {stat.value}
            </Typography>
          </Box>
        ))}
      </Stack>
      {totals.hasUnknownCost && (
        <Typography variant="caption" color="text.secondary">
          Cost shown is partial — some requests used a model without known pricing, so the real total is at least this much.
        </Typography>
      )}

      <Box sx={{ p: 2, borderRadius: 2, border: 1, borderColor: 'divider' }}>
        {loading ? (
          <Typography variant="caption" color="text.secondary">
            Loading…
          </Typography>
        ) : (
          <>
            <Box sx={{ display: 'inline-grid', gridAutoFlow: 'column', gap: '3px', gridTemplateRows: 'repeat(7, 11px)' }}>
              {cells.map((cell) => (
                <Box
                  key={cell.date}
                  onMouseEnter={() => cell.day && setHovered({ date: cell.date, day: cell.day })}
                  onMouseLeave={() => setHovered(null)}
                  sx={{
                    width: 11,
                    height: 11,
                    borderRadius: '2px',
                    bgcolor: cell.day ? 'primary.main' : 'action.hover',
                    opacity: cell.day ? Math.max(0.25, (cell.day.input + cell.day.output) / (maxTotal || 1)) : 1,
                    cursor: cell.day ? 'pointer' : 'default',
                  }}
                />
              ))}
            </Box>
            {hovered && (
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>
                <Box component="span" sx={{ color: 'text.primary', fontWeight: 600 }}>
                  {hovered.date}
                </Box>{' '}
                — {hovered.day.requests} request{hovered.day.requests === 1 ? '' : 's'},{' '}
                {(hovered.day.input + hovered.day.output).toLocaleString()} tokens,{' '}
                {hovered.day.cost === 0 && hovered.day.hasUnknownCost
                  ? 'cost unknown'
                  : `$${hovered.day.cost.toFixed(4)}${hovered.day.hasUnknownCost ? '+' : ''}`}
              </Typography>
            )}
          </>
        )}
        {!loading && maxTotal === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>
            No usage recorded yet — this fills in as you use real provider models.
          </Typography>
        )}
      </Box>
    </Stack>
  )
}
