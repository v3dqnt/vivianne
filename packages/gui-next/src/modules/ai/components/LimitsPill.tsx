import { useEffect, useState } from 'react'
import { Box, Button, ButtonBase, Popover, Stack, TextField, Typography } from '../../../ui'
import { ClockLinear } from '../../../icons'
import { call } from '../../../ipc'

interface RunLimits {
  capUsd: number | null
  capHours: number | null
}

/** "$20 · 3d" — limits as the pill shows them. */
export function limitsLabel({ capUsd, capHours }: RunLimits): string {
  const parts: string[] = []
  if (capUsd !== null) parts.push(`$${capUsd % 1 === 0 ? capUsd : capUsd.toFixed(2)}`)
  if (capHours !== null) parts.push(capHours >= 24 && capHours % 24 === 0 ? `${capHours / 24}d` : `${capHours}h`)
  return parts.join(' · ')
}

const parseLimit = (raw: string): number | null => {
  const n = Number(raw.replace(/[$,\s]/g, ''))
  return Number.isFinite(n) && n > 0 ? n : null
}

/**
 * Per-task spending and time limits, for working within a fixed amount of
 * credit. Off by default: a task normally runs until it is done. With a
 * limit set, a task that reaches it pauses and says so, and can be resumed
 * with more.
 *
 * Saved to settings (`runLimits`) and applied to every new task until
 * changed — a budget is a standing decision, not a per-message one.
 */
export function LimitsPill() {
  const [limits, setLimits] = useState<RunLimits>({ capUsd: null, capHours: null })
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [usd, setUsd] = useState('')
  const [hours, setHours] = useState('')

  useEffect(() => {
    void (async () => {
      const cfg = await call<Record<string, unknown>>('cmd_get_settings')
      const l = (cfg?.runLimits ?? {}) as Partial<RunLimits>
      setLimits({ capUsd: l.capUsd ?? null, capHours: l.capHours ?? null })
    })()
  }, [])

  const open = (el: HTMLElement) => {
    setUsd(limits.capUsd?.toString() ?? '')
    setHours(limits.capHours?.toString() ?? '')
    setAnchor(el)
  }

  const save = async (next: RunLimits) => {
    // Re-read first: settings is one blob, and it may have changed since
    // this pill loaded.
    const cfg = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
    await call('cmd_save_settings', { settings: { ...cfg, runLimits: next } })
    setLimits(next)
    setAnchor(null)
  }

  const label = limitsLabel(limits)
  return (
    <>
      <ButtonBase
        onClick={(e) => open(e.currentTarget)}
        title={label ? `Each task stops at ${label} — click to change` : 'Set a budget or time limit per task'}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 0.75,
          px: 1,
          py: 0.375,
          borderRadius: '8px',
          border: 1,
          borderColor: 'divider',
          color: label ? 'text.primary' : 'text.secondary',
        }}
      >
        <ClockLinear size={13} color="currentColor" />
        <Typography className="cq-label" variant="caption" sx={{ fontWeight: 500 }}>
          {label || 'No limits'}
        </Typography>
      </ButtonBase>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
      >
        <Stack spacing={1.5} sx={{ p: 2, width: 300 }}>
          <Box>
            <Typography variant="body2" sx={{ fontWeight: 500 }}>
              Limits per task
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 0.5, lineHeight: 1.5 }}>
              Tasks run until they're done. Set a limit to work within a budget: a task that reaches it pauses and tells
              you, and you can resume it with more.
            </Typography>
          </Box>
          <Stack direction="row" spacing={1}>
            <TextField label="Budget (USD)" size="small" value={usd} onChange={(e) => setUsd(e.target.value)} placeholder="No limit" />
            <TextField label="Time (hours)" size="small" value={hours} onChange={(e) => setHours(e.target.value)} placeholder="No limit" />
          </Stack>
          <Stack direction="row" spacing={1} sx={{ justifyContent: 'flex-end' }}>
            {label && (
              <Button size="small" color="inherit" onClick={() => void save({ capUsd: null, capHours: null })}>
                Remove limits
              </Button>
            )}
            <Button size="small" variant="contained" onClick={() => void save({ capUsd: parseLimit(usd), capHours: parseLimit(hours) })}>
              Save
            </Button>
          </Stack>
        </Stack>
      </Popover>
    </>
  )
}
