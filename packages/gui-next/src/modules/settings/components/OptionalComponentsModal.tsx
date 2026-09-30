import { useEffect } from 'react'
import { Dialog, DialogTitle, DialogContent, DialogActions, Typography, Button, Stack } from '../../../ui'
import { hasTauri } from '../../../ipc'
import { useOptionalComponentsStore } from '../store/optionalComponentsStore'

/** Shown once, on first launch (persisted `optionalComponentsPrompted` flag —
 * either answer counts, so this never nags twice). Both downloads run in the
 * background afterward regardless of which button is pressed here; "Download
 * now" just starts them immediately instead of leaving them for whichever
 * tool call needs one first. Real progress for both shows in Settings →
 * Components either way. */
export function OptionalComponentsModal() {
  const init = useOptionalComponentsStore((s) => s.init)
  const prompted = useOptionalComponentsStore((s) => s.prompted)
  const dismissPrompt = useOptionalComponentsStore((s) => s.dismissPrompt)
  const startDownload = useOptionalComponentsStore((s) => s.startDownload)

  useEffect(() => {
    void init()
  }, [init])

  if (!hasTauri() || prompted) return null

  return (
    <Dialog open maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 600 }}>Optional components</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.5 }}>
          <Typography variant="body2" color="text.secondary">
            Two things Vivianne can use aren&apos;t installed yet:
          </Typography>
          <Typography variant="body2" color="text.secondary" component="ul" sx={{ pl: 2.5, m: 0 }}>
            <li>a browser engine (~300MB), for web browsing and previews</li>
            <li>Docker (~500MB), for running untrusted code in a sandbox</li>
          </Typography>
          <Typography variant="body2" color="text.secondary">
            Neither is required for coding tasks. Download them now in the
            background, or skip for now and install either one later from
            Settings → Components whenever you actually need it.
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2.5 }}>
        <Button onClick={() => void dismissPrompt()} color="inherit">
          Later
        </Button>
        <Button onClick={() => void startDownload()} variant="contained" disableElevation>
          Download now
        </Button>
      </DialogActions>
    </Dialog>
  )
}
