import { useEffect } from 'react'
import Stack from '@mui/material/Stack'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Button from '@mui/material/Button'
import LinearProgress from '@mui/material/LinearProgress'
import Divider from '@mui/material/Divider'
import CircularProgress from '@mui/material/CircularProgress'
import { useUpdateStore } from '../../updates/store/updateStore'
import { ChangelogBody } from '../../updates/components/ChangelogBody'

/** Settings → App → Updates: what's running, whether anything newer exists,
 * and the full release history. The update check is manual here; a silent
 * one also runs at startup (see `App.tsx`). */
export function UpdatesSection() {
  const version = useUpdateStore((s) => s.version)
  const changelog = useUpdateStore((s) => s.changelog)
  const update = useUpdateStore((s) => s.update)
  const checkStatus = useUpdateStore((s) => s.checkStatus)
  const installStatus = useUpdateStore((s) => s.installStatus)
  const progress = useUpdateStore((s) => s.progress)
  const error = useUpdateStore((s) => s.error)
  const check = useUpdateStore((s) => s.check)
  const install = useUpdateStore((s) => s.install)
  const init = useUpdateStore((s) => s.init)

  useEffect(() => {
    if (!version) void init()
  }, [version, init])

  const pct =
    progress && progress.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null

  return (
    <Stack sx={{ gap: 2.5 }}>
      <Stack
        direction="row"
        sx={{ alignItems: 'center', gap: 2, p: 2, borderRadius: 1.2, border: 1, borderColor: 'divider' }}
      >
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Vivianne {version || '—'}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {checkStatus === 'checking'
              ? 'Checking for updates…'
              : checkStatus === 'available' && update
                ? `Version ${update.version} is available.`
                : checkStatus === 'up-to-date'
                  ? "You're on the latest version."
                  : checkStatus === 'error'
                    ? (error ?? 'Update check failed.')
                    : 'Updates install from the official release feed.'}
          </Typography>
        </Stack>

        {update ? (
          <Button
            variant="contained"
            disableElevation
            onClick={() => void install()}
            disabled={installStatus === 'installing'}
            sx={{ textTransform: 'none', flexShrink: 0 }}
          >
            {installStatus === 'installing' ? 'Installing…' : `Update to ${update.version}`}
          </Button>
        ) : (
          <Button
            variant="outlined"
            onClick={() => void check()}
            disabled={checkStatus === 'checking'}
            startIcon={checkStatus === 'checking' ? <CircularProgress size={13} /> : undefined}
            sx={{ textTransform: 'none', flexShrink: 0 }}
          >
            Check for updates
          </Button>
        )}
      </Stack>

      {installStatus === 'installing' && (
        <Box>
          <LinearProgress variant={pct === null ? 'indeterminate' : 'determinate'} value={pct ?? 0} sx={{ borderRadius: 1 }} />
          <Typography variant="caption" color="text.secondary" sx={{ mt: 0.75, display: 'block' }}>
            {pct === null
              ? 'Downloading…'
              : `Downloading… ${pct}%`}{' '}
            The app restarts on its own once this finishes.
          </Typography>
        </Box>
      )}

      {update?.notes && (
        <Box sx={{ p: 2, borderRadius: 1.2, border: 1, borderColor: 'divider' }}>
          <Typography variant="body2" sx={{ fontWeight: 600, mb: 1 }}>
            What's in {update.version}
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ whiteSpace: 'pre-wrap' }}>
            {update.notes}
          </Typography>
        </Box>
      )}

      <Divider />

      <Stack sx={{ gap: 1 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Release history
        </Typography>
        {changelog.length === 0 ? (
          <Typography variant="caption" color="text.secondary">
            No changelog entries found.
          </Typography>
        ) : (
          <ChangelogBody entries={changelog} />
        )}
      </Stack>
    </Stack>
  )
}
