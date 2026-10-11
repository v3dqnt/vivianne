import { useEffect } from 'react'
import { Stack, Box, Typography, ButtonBase } from '../../../ui'
import { CloseLinear } from '../../../icons'
import { hasTauri } from '../../../ipc'
import { useProjectsStore } from '../../sessions/store/projectsStore'
import { useModsStore } from '../store/modsStore'
import { useToastStore } from '../store/toastStore'

/** How often mods' files are checked for edits (open pages reload). */
const WATCH_MS = 2500

/**
 * Mounted once in the app: lists mods for the open project (again whenever
 * it changes), reloads mod pages when their files change, and shows toasts.
 */
export function ModsRuntime() {
  const projectPath = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId)?.path ?? null)
  const refresh = useModsStore((s) => s.refresh)
  const pollChanges = useModsStore((s) => s.pollChanges)
  const toasts = useToastStore((s) => s.items)
  const dismiss = useToastStore((s) => s.dismiss)

  useEffect(() => {
    if (!hasTauri()) return
    void refresh(projectPath)
  }, [projectPath, refresh])

  useEffect(() => {
    if (!hasTauri()) return
    void pollChanges()
    const t = window.setInterval(() => {
      if (document.visibilityState === 'visible') void pollChanges()
    }, WATCH_MS)
    return () => window.clearInterval(t)
  }, [pollChanges])

  if (toasts.length === 0) return null
  return (
    <Stack sx={{ position: 'fixed', right: 16, bottom: 16, gap: 1, zIndex: 2000, maxWidth: 360 }}>
      {toasts.map((t) => (
        <Box
          key={t.id}
          role="status"
          sx={{
            display: 'flex',
            alignItems: 'flex-start',
            gap: 1,
            px: 1.5,
            py: 1,
            borderRadius: 2,
            bgcolor: 'background.paper',
            border: 1,
            borderColor: 'divider',
            boxShadow: 3,
          }}
        >
          <Typography variant="body2" sx={{ flex: 1, fontSize: '0.8125rem' }}>
            {t.text}
          </Typography>
          <ButtonBase aria-label="Dismiss" onClick={() => dismiss(t.id)} sx={{ borderRadius: 1, color: 'text.secondary' }}>
            <CloseLinear size={14} color="currentColor" />
          </ButtonBase>
        </Box>
      ))}
    </Stack>
  )
}
