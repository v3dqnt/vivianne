import { Button, Dialog, DialogActions, DialogContent, DialogTitle, Typography } from '../../ui'
import { useWorkflowAsk } from './workflowRunner'

/** A workflow step that asked to be confirmed first. */
export function WorkflowConfirm() {
  const pending = useWorkflowAsk((s) => s.pending)
  return (
    <Dialog open={!!pending} onClose={() => pending?.answer(false)} maxWidth="xs" fullWidth>
      <DialogTitle>
        {pending?.workflow}: step {pending?.step} of {pending?.total}
      </DialogTitle>
      <DialogContent>
        <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', color: 'text.secondary' }}>
          {pending?.text}
        </Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => pending?.answer(false)} sx={{ textTransform: 'none' }}>
          Skip the rest
        </Button>
        <Button variant="contained" onClick={() => pending?.answer(true)} sx={{ textTransform: 'none' }}>
          Run this step
        </Button>
      </DialogActions>
    </Dialog>
  )
}
