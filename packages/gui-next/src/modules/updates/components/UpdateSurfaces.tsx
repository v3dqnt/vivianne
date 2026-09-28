import { useEffect } from 'react'
import { Dialog, DialogTitle, DialogContent, DialogActions, Button, Stack, Box, Typography, IconButton, Tooltip, useTheme } from '../../../ui'
import { CloseLinear } from '../../../icons'
import { surfaceCard } from '../../../theme'
import { hasTauri } from '../../../ipc'
import { on } from '../../../ipc'
import { useUpdateStore } from '../store/updateStore'
import { ChangelogBody } from './ChangelogBody'

/**
 * Everything the update system shows outside Settings:
 *
 * - a one-time "what's new" dialog after the app updates itself,
 * - a dismissible strip for remote announcements,
 * - an unobtrusive prompt when a newer build is available.
 *
 * Mounted once from `App.tsx`. Also owns startup init and the silent update
 * check, so nothing else has to remember to kick those off.
 */
export function UpdateSurfaces() {
  const theme = useTheme()
  const init = useUpdateStore((s) => s.init)
  const check = useUpdateStore((s) => s.check)
  const setProgress = useUpdateStore((s) => s.setProgress)
  const showWhatsNew = useUpdateStore((s) => s.showWhatsNew)
  const dismissWhatsNew = useUpdateStore((s) => s.dismissWhatsNew)
  const version = useUpdateStore((s) => s.version)
  const changelog = useUpdateStore((s) => s.changelog)
  const announcements = useUpdateStore((s) => s.announcements)
  const dismissAnnouncement = useUpdateStore((s) => s.dismissAnnouncement)
  const update = useUpdateStore((s) => s.update)
  const installStatus = useUpdateStore((s) => s.installStatus)
  const install = useUpdateStore((s) => s.install)

  useEffect(() => {
    if (!hasTauri()) return
    void init()
    // Silent: a failed check on a machine that's offline, or behind a proxy
    // that blocks the release host, must not greet the user with an error.
    void check({ silent: true })

    let unlisten: (() => void) | null = null
    void on<{ downloaded: number; total: number | null }>('update-progress', (p) => setProgress(p)).then((fn) => {
      unlisten = fn
    })
    return () => unlisten?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Only this build's entry — the dialog is "what changed in the version you
  // just got", not the full history (that's Settings → Updates).
  const currentEntry = changelog.find((e) => e.version === version)

  return (
    <>
      <Dialog
        open={showWhatsNew && !!currentEntry}
        onClose={() => void dismissWhatsNew()}
        maxWidth="sm"
        fullWidth
        slotProps={{ paper: { sx: { backgroundImage: 'none', ...surfaceCard(theme.palette.mode) } } }}
      >
        <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>What's new in {version}</DialogTitle>
        <DialogContent dividers>{currentEntry && <ChangelogBody entries={[currentEntry]} />}</DialogContent>
        <DialogActions>
          <Button variant="contained" disableElevation onClick={() => void dismissWhatsNew()} sx={{ textTransform: 'none' }}>
            Got it
          </Button>
        </DialogActions>
      </Dialog>

      {/* Announcements + update prompt share one strip along the bottom so
          they can't stack into competing banners. */}
      {(announcements.length > 0 || (update && installStatus !== 'installing')) && (
        <Stack
          sx={{
            position: 'fixed',
            bottom: 12,
            right: 12,
            zIndex: (t) => t.zIndex.snackbar,
            gap: 1,
            maxWidth: 380,
          }}
        >
          {announcements.map((a) => (
            <Stack
              key={a.id}
              direction="row"
              sx={{ ...surfaceCard(theme.palette.mode), p: 1.5, gap: 1, alignItems: 'flex-start' }}
            >
              <Box
                sx={{
                  width: 3,
                  alignSelf: 'stretch',
                  borderRadius: 3,
                  flexShrink: 0,
                  bgcolor:
                    a.level === 'warning' ? 'warning.main' : a.level === 'success' ? 'success.main' : 'primary.main',
                }}
              />
              <Stack sx={{ flex: 1, minWidth: 0, gap: 0.25 }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  {a.title}
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  {a.body}
                </Typography>
                {a.url && (
                  <Typography
                    component="a"
                    href={a.url}
                    target="_blank"
                    rel="noreferrer"
                    variant="caption"
                    sx={{ color: 'primary.main', textDecoration: 'none', '&:hover': { textDecoration: 'underline' } }}
                  >
                    Learn more
                  </Typography>
                )}
              </Stack>
              <Tooltip title="Dismiss">
                <IconButton size="small" onClick={() => void dismissAnnouncement(a.id)} sx={{ width: 22, height: 22 }}>
                  <CloseLinear size={12} color="currentColor" />
                </IconButton>
              </Tooltip>
            </Stack>
          ))}

          {update && installStatus !== 'installing' && (
            <Stack direction="row" sx={{ ...surfaceCard(theme.palette.mode), p: 1.5, gap: 1, alignItems: 'center' }}>
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  Version {update.version} is available
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  You're on {update.currentVersion}.
                </Typography>
              </Stack>
              <Button size="small" variant="contained" disableElevation onClick={() => void install()} sx={{ textTransform: 'none' }}>
                Update
              </Button>
            </Stack>
          )}
        </Stack>
      )}
    </>
  )
}
