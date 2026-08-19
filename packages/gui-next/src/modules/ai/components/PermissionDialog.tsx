import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import Button from '@mui/material/Button'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { ShieldTickLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'

/**
 * The backend already gates tool calls behind `PermissionAsked`/`PermissionResolved`
 * events (see `agent/events.rs`) whenever bypass mode is off — this dialog was the
 * missing piece: without it, `agentStore` dropped the event entirely and the turn
 * just sat there waiting on a response that could never come, which is also the
 * likely explanation for "send button stays busy" reports.
 */
export function PermissionDialog() {
  const pending = useAgentStore((s) => s.pendingPermission)
  const respond = useAgentStore((s) => s.respondPermission)

  if (!pending) return null

  const argsText = (() => {
    try {
      return JSON.stringify(pending.args, null, 2)
    } catch {
      return String(pending.args)
    }
  })()

  return (
    <Dialog open onClose={() => void respond(false)} maxWidth="sm" fullWidth>
      <DialogTitle>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: 'warning.main' }}>
            <ShieldTickLinear size={20} color="currentColor" />
          </Box>
          <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
            Run {pending.toolName}?
          </Typography>
        </Stack>
      </DialogTitle>
      <DialogContent>
        {pending.reason && (
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5 }}>
            {pending.reason}
          </Typography>
        )}
        <Box
          component="pre"
          sx={{
            m: 0,
            p: 1.5,
            borderRadius: 2,
            bgcolor: 'action.hover',
            fontFamily: '"JetBrains Mono", ui-monospace, monospace',
            fontSize: '0.75rem',
            lineHeight: 1.5,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            maxHeight: 240,
            overflowY: 'auto',
          }}
        >
          {argsText}
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => void respond(false)} color="inherit">
          Deny
        </Button>
        <Button onClick={() => void respond(true)} variant="contained" autoFocus>
          Allow
        </Button>
      </DialogActions>
    </Dialog>
  )
}
