import { useState } from 'react'
import { Dialog, DialogTitle, DialogContent, DialogActions, Button, Typography, Box } from '../../../ui'
import { Markdown } from '../../ai/components/Markdown'
import { call } from '../../../ipc'
import type { MasterInfo } from '../store/masterStore'

/**
 * The project's charter, as the master agent sees it.
 *
 * Read-only here on purpose. The charter is a file the master also edits as it
 * records decisions, and a second editor with its own copy of the text would
 * race it — whichever saved last would silently erase the other's changes.
 * "Open file" hands the real file to your own Markdown editor instead.
 */
export function CharterDialog({ master, open, onClose }: { master: MasterInfo; open: boolean; onClose: () => void }) {
  const [error, setError] = useState<string | null>(null)
  const openFile = async () => {
    setError(null)
    try {
      await call('cmd_open_agent_file', { path: master.charterPath })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle sx={{ pb: 0.5 }}>
        <Typography component="span" sx={{ fontSize: '1rem', fontWeight: 700, display: 'block' }}>
          {master.name}’s charter
        </Typography>
        <Typography
          component="span"
          variant="caption"
          color="text.secondary"
          sx={{ display: 'block', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', mt: 0.25 }}
        >
          {master.charterPath}
        </Typography>
      </DialogTitle>
      <DialogContent dividers>
        <Box sx={{ fontSize: '0.875rem' }}>
          <Markdown text={master.charter} />
        </Box>
      </DialogContent>
      <DialogActions sx={{ px: 3 }}>
        <Typography variant="caption" color="text.secondary" sx={{ mr: 'auto' }}>
          {error ?? 'The master updates this as decisions are made.'}
        </Typography>
        <Button color="inherit" onClick={onClose}>
          Close
        </Button>
        <Button variant="contained" disableElevation onClick={() => void openFile()}>
          Open file
        </Button>
      </DialogActions>
    </Dialog>
  )
}
