import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import { CloseCircleLinear } from '../../../icons'
import { useSessionsStore } from '../store/sessionsStore'
import { useThreadsStore } from '../store/threadsStore'
import { SESSION_DRAG_MIME } from './ChatDockZone'

/**
 * Tabs for open threads, above the chat. Also a drop target — dragging a
 * session here opens it as a tab, same as dropping into the side pane zone,
 * so the drag gesture works toward either target rather than only the narrow
 * right-hand strip.
 */
export function ThreadTabBar() {
  const openIds = useThreadsStore((s) => s.openIds)
  const focusedId = useThreadsStore((s) => s.focusedId)
  const open = useThreadsStore((s) => s.open)
  const close = useThreadsStore((s) => s.close)
  const focus = useThreadsStore((s) => s.focus)
  const sessions = useSessionsStore((s) => s.sessions)
  const activeSessionId = useSessionsStore((s) => s.activeSessionId)
  const loadSession = useSessionsStore((s) => s.loadSession)

  const nameOf = (id: string) => sessions.find((s) => s.id === id)?.name || 'Untitled'

  if (openIds.length === 0) return null

  return (
    <Stack
      direction="row"
      onDragOver={(e) => {
        e.preventDefault()
        e.dataTransfer.dropEffect = 'copy'
      }}
      onDrop={(e) => {
        e.preventDefault()
        const id = e.dataTransfer.getData(SESSION_DRAG_MIME) || e.dataTransfer.getData('text/plain')
        if (id) open(id)
      }}
      sx={{
        alignItems: 'center',
        gap: 0.5,
        px: 1,
        py: 0.5,
        flexShrink: 0,
        overflowX: 'auto',
        borderBottom: 1,
        borderColor: 'divider',
      }}
    >
      {openIds.map((id) => {
        const isFocused = id === focusedId
        return (
          <Stack
            key={id}
            direction="row"
            onClick={() => {
              focus(id)
              if (activeSessionId !== id) void loadSession(id)
            }}
            sx={{
              alignItems: 'center',
              gap: 0.5,
              px: 1,
              py: 0.375,
              borderRadius: 1.5,
              cursor: 'pointer',
              flexShrink: 0,
              maxWidth: 180,
              bgcolor: isFocused ? 'action.selected' : 'transparent',
              border: 1,
              borderColor: isFocused ? 'primary.main' : 'transparent',
              transition: 'background-color 0.12s, border-color 0.12s',
              '&:hover': { bgcolor: 'action.hover' },
            }}
          >
            <Typography
              variant="caption"
              noWrap
              title={nameOf(id)}
              sx={{ minWidth: 0, fontWeight: isFocused ? 600 : 400, color: isFocused ? 'text.primary' : 'text.secondary' }}
            >
              {nameOf(id)}
            </Typography>
            <IconButton
              size="small"
              onClick={(e) => {
                e.stopPropagation()
                close(id)
              }}
              sx={{ p: 0.125, flexShrink: 0 }}
            >
              <CloseCircleLinear size={12} color="currentColor" />
            </IconButton>
          </Stack>
        )
      })}
      <Box sx={{ flex: 1 }} />
    </Stack>
  )
}
