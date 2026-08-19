import { useState } from 'react'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import IconButton from '@mui/material/IconButton'
import TextField from '@mui/material/TextField'
import Typography from '@mui/material/Typography'
import CircularProgress from '@mui/material/CircularProgress'
import { ArrowLeftLinear, ArrowRightLinear, RefreshLinear, CloseCircleLinear, GlobalLinear } from '../../../icons'
import { useWebPreviewStore } from '../store/webPreviewStore'

/** Opened by the `web_preview` tool via `AgentEvent::WebPreviewRequested`
 * (routed through `agentStore`). `screenshotBase64` is a real render from
 * camoufox-browser — a separate browser process, not an iframe — so it works
 * even for sites that block framing. */
export function WebPreviewPanel() {
  const open = useWebPreviewStore((s) => s.open)
  const url = useWebPreviewStore((s) => s.url)
  const screenshotBase64 = useWebPreviewStore((s) => s.screenshotBase64)
  const error = useWebPreviewStore((s) => s.error)
  const loading = useWebPreviewStore((s) => s.loading)
  const close = useWebPreviewStore((s) => s.close)
  const back = useWebPreviewStore((s) => s.back)
  const forward = useWebPreviewStore((s) => s.forward)
  const refresh = useWebPreviewStore((s) => s.refresh)
  const navigate = useWebPreviewStore((s) => s.navigate)
  const [addressDraft, setAddressDraft] = useState('')

  if (!open) return null

  return (
    <Dialog open={open} onClose={close} maxWidth="md" fullWidth slotProps={{ paper: { sx: { height: '80vh' } } }}>
      <DialogTitle sx={{ p: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
          <IconButton size="small" onClick={() => void back()} disabled={loading}>
            <ArrowLeftLinear size={16} color="currentColor" />
          </IconButton>
          <IconButton size="small" onClick={() => void forward()} disabled={loading}>
            <ArrowRightLinear size={16} color="currentColor" />
          </IconButton>
          <IconButton size="small" onClick={() => void refresh()} disabled={loading}>
            {loading ? <CircularProgress size={14} /> : <RefreshLinear size={15} color="currentColor" />}
          </IconButton>
          <TextField
            size="small"
            fullWidth
            value={addressDraft || url}
            onChange={(e) => setAddressDraft(e.target.value)}
            onFocus={() => setAddressDraft(url)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && addressDraft.trim()) void navigate(addressDraft.trim())
            }}
            slotProps={{ input: { startAdornment: <GlobalLinear size={13} color="currentColor" style={{ marginRight: 6 }} /> } }}
            sx={{ '& .MuiInputBase-root': { fontSize: '0.8125rem', fontFamily: 'monospace' } }}
          />
          <IconButton size="small" onClick={close}>
            <CloseCircleLinear size={16} color="currentColor" />
          </IconButton>
        </Stack>
      </DialogTitle>
      <DialogContent sx={{ p: 0, display: 'flex', flexDirection: 'column', bgcolor: 'action.hover' }}>
        {error ? (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', p: 3 }}>
            <Typography variant="body2" color="error.main">
              {error}
            </Typography>
          </Stack>
        ) : screenshotBase64 ? (
          <Box
            component="img"
            src={`data:image/png;base64,${screenshotBase64}`}
            sx={{ width: '100%', flex: 1, objectFit: 'contain', bgcolor: 'background.default' }}
          />
        ) : (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
            <CircularProgress size={20} />
          </Stack>
        )}
      </DialogContent>
    </Dialog>
  )
}
