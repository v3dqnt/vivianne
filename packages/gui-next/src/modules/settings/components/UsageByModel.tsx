import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, ToggleButton, ToggleButtonGroup, Tooltip, Typography } from '../../../ui'
import { call } from '../../../ipc'

/** Mirrors `usage_store::DayUsage`, summed per model. */
interface ModelUsage {
  input: number
  output: number
  cost: number
  requests: number
  hasUnknownCost: boolean
}

const RANGES = [
  { id: '7', label: '7 days', days: 7 },
  { id: '30', label: '30 days', days: 30 },
  { id: 'all', label: 'All', days: null },
] as const

const compact = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n))
const money = (c: number) => `$${c < 0.1 ? c.toFixed(3) : c.toFixed(2)}`

/** "provider/vendor/model" → the provider and a readable model name. */
function split(key: string): { provider: string; model: string } {
  const i = key.indexOf('/')
  return i < 0 ? { provider: '', model: key } : { provider: key.slice(0, i), model: key.slice(i + 1) }
}

/**
 * Which model did the work, and what it cost: tokens and spend per model,
 * from the same real provider-reported usage as the rest of this page. Models
 * without a known price are marked, not shown as free.
 */
export function UsageByModel() {
  const [range, setRange] = useState<(typeof RANGES)[number]['id']>('30')
  const [data, setData] = useState<Record<string, ModelUsage> | null>(null)

  useEffect(() => {
    let active = true
    const days = RANGES.find((r) => r.id === range)?.days ?? null
    void call<Record<string, ModelUsage>>('cmd_get_usage_by_model', { days }).then((d) => active && setData(d ?? {}))
    return () => {
      active = false
    }
  }, [range])

  const rows = useMemo(() => {
    const list = Object.entries(data ?? {}).map(([key, u]) => ({ key, ...split(key), ...u }))
    return list.sort((a, b) => b.input + b.output - (a.input + a.output))
  }, [data])
  const total = rows.reduce((t, r) => ({ tokens: t.tokens + r.input + r.output, cost: t.cost + r.cost, unknown: t.unknown || r.hasUnknownCost }), { tokens: 0, cost: 0, unknown: false })
  const max = Math.max(1, ...rows.map((r) => r.input + r.output))

  return (
    <Box sx={{ p: 2, borderRadius: 1.2, border: 1, borderColor: 'divider' }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          By model
        </Typography>
        <ToggleButtonGroup size="small" exclusive value={range} onChange={(_: unknown, v: typeof range | null) => v && setRange(v)}>
          {RANGES.map((r) => (
            <ToggleButton key={r.id} value={r.id} sx={{ textTransform: 'none', px: 1, py: 0.125, fontSize: '0.75rem' }}>
              {r.label}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
      </Stack>

      {data === null ? (
        <Typography variant="caption" color="text.secondary">
          Loading…
        </Typography>
      ) : rows.length === 0 ? (
        <Typography variant="caption" color="text.secondary">
          Nothing in this range yet. Per-model usage starts counting from now.
        </Typography>
      ) : (
        <Stack sx={{ gap: 1 }}>
          {rows.map((r) => (
            <Box key={r.key}>
              <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
                <Typography variant="body2" sx={{ fontWeight: 500, minWidth: 0, flex: 1 }} noWrap title={r.key}>
                  {r.model}
                  {r.provider && (
                    <Typography component="span" variant="caption" color="text.disabled" sx={{ ml: 0.75 }}>
                      {r.provider}
                    </Typography>
                  )}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', flexShrink: 0 }}>
                  {compact(r.input)} in · {compact(r.output)} out
                </Typography>
                <Tooltip title={r.hasUnknownCost ? 'This model has no known price, so its cost is partial or unknown' : 'Cost at the model’s listed price'}>
                  <Typography
                    variant="body2"
                    sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', width: 76, textAlign: 'right', flexShrink: 0, color: r.hasUnknownCost ? 'text.disabled' : 'text.primary' }}
                  >
                    {r.hasUnknownCost && r.cost === 0 ? 'unpriced' : `${money(r.cost)}${r.hasUnknownCost ? '+' : ''}`}
                  </Typography>
                </Tooltip>
              </Stack>
              <Box sx={{ mt: 0.5, height: 4, borderRadius: 2, bgcolor: 'action.hover', overflow: 'hidden' }}>
                <Box sx={{ width: `${Math.max(2, ((r.input + r.output) / max) * 100)}%`, height: '100%', bgcolor: 'primary.main' }} />
              </Box>
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
                {r.requests} request{r.requests === 1 ? '' : 's'}
              </Typography>
            </Box>
          ))}
          <Stack direction="row" sx={{ justifyContent: 'space-between', pt: 1, borderTop: 1, borderColor: 'divider' }}>
            <Typography variant="caption" color="text.secondary">
              {rows.length} model{rows.length === 1 ? '' : 's'} · {compact(total.tokens)} tokens
            </Typography>
            <Typography variant="caption" sx={{ fontWeight: 600, fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
              {money(total.cost)}
              {total.unknown ? '+' : ''}
            </Typography>
          </Stack>
        </Stack>
      )}
    </Box>
  )
}
