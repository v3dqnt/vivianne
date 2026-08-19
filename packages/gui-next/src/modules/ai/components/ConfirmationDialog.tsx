import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import Button from '@mui/material/Button'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { DangerLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'

const DANGER_COLOR: Record<string, 'info' | 'warning' | 'error'> = {
  low: 'info',
  medium: 'warning',
  high: 'error',
}

/** The `confirm_action` tool's explicit "are you sure?" — distinct from
 * `PermissionDialog`, which gates a fixed list of tools; this fires whenever
 * the model itself decides a specific action deserves a check-in. */
export function ConfirmationDialog() {
  const pending = useAgentStore((s) => s.pendingConfirmation)
  const respond = useAgentStore((s) => s.respondConfirmation)

  if (!pending) return null

  const color = DANGER_COLOR[pending.dangerLevel] ?? 'warning'

  return (
    <Dialog open onClose={() => void respond(false)} maxWidth="xs" fullWidth>
      <DialogTitle>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: `${color}.main` }}>
            <DangerLinear size={20} color="currentColor" />
          </Box>
          <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
            Confirm action
          </Typography>
        </Stack>
      </DialogTitle>
      <DialogContent>
        <Typography variant="body2">{pending.message}</Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => void respond(false)} color="inherit">
          No
        </Button>
        <Button onClick={() => void respond(true)} variant="contained" color={color} autoFocus>
          Yes
        </Button>
      </DialogActions>
    </Dialog>
  )
}
