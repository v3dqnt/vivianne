import { useEffect, useState } from 'react'
import { Box, Stack, Typography, Button, alpha, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import type { InstantRisk } from '../store/instantStore'

/** Matches the backend's tick, so the bar and the real deadline stay in step. */
const TICK_MS = 100

/**
 * The three seconds before a destructive step runs.
 *
 * This is the only place Viv Instant pauses. Everything else executes without
 * asking — that is what the feature was asked to be — but `dip`'s clicks and
 * keystrokes have no undo anywhere in this app, so a step the classifier reads
 * as destructive gets a window in which the user can stop it.
 *
 * It counts down to "yes" rather than waiting for one. A prompt would strand an
 * unattended run the moment the user looked away, which is most of the time;
 * this keeps the run moving while still making "no" reachable.
 */
export function InstantRiskBar({ risk }: { risk: InstantRisk }) {
  const theme = useTheme()
  const [remaining, setRemaining] = useState(() => Math.max(0, risk.deadline - Date.now()))

  useEffect(() => {
    // Recomputed from the absolute deadline on every tick rather than
    // decremented, so a throttled timer in a background window shows the true
    // time left instead of drifting behind the backend's countdown.
    const id = setInterval(() => setRemaining(Math.max(0, risk.deadline - Date.now())), TICK_MS)
    return () => clearInterval(id)
  }, [risk.deadline])

  const fraction = Math.min(1, Math.max(0, remaining / Math.max(1, risk.ms)))
  const seconds = Math.ceil(remaining / 1000)

  return (
    <Stack
      sx={{
        gap: 0.5,
        p: 0.875,
        borderRadius: '10px',
        bgcolor: alpha(theme.palette.warning.main, 0.14),
        border: 1,
        borderColor: alpha(theme.palette.warning.main, 0.4),
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Typography variant="caption" sx={{ fontWeight: 700, color: 'warning.main', flexShrink: 0 }}>
          {seconds}s
        </Typography>
        <Typography
          variant="caption"
          sx={{ flex: 1, minWidth: 0, color: 'text.primary', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
        >
          {risk.summary}
        </Typography>
        {/* Named because the voice route is the one that works when the user
            is looking at the app being driven rather than at this overlay. */}
        <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem', flexShrink: 0 }}>
          or say “stop”
        </Typography>
        <Button
          size="small"
          variant="contained"
          color="warning"
          onClick={() => void call('cmd_instant_cancel_step', {})}
          sx={{ textTransform: 'none', fontSize: '0.75rem', py: 0.125, minWidth: 0, flexShrink: 0 }}
        >
          Cancel
        </Button>
      </Stack>
      {/* A plain scaled div rather than LinearProgress: the value changes every
          100ms, and MUI's transition would lag visibly behind the number. */}
      <Box sx={{ height: 3, borderRadius: 2, bgcolor: alpha(theme.palette.warning.main, 0.2), overflow: 'hidden' }}>
        <Box
          sx={{
            height: '100%',
            width: '100%',
            bgcolor: 'warning.main',
            transformOrigin: 'left',
            transform: `scaleX(${fraction})`,
          }}
        />
      </Box>
    </Stack>
  )
}
