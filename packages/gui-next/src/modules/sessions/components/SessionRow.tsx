import { useState, type KeyboardEvent } from 'react'
import Box from '@mui/material/Box'
import ListItemButton from '@mui/material/ListItemButton'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import { EditLinear, TrashLinear, TickCircleLinear } from 'mx-icons'
import type { SessionInfo } from '../lib/types'
import { useSessionsStore } from '../store/sessionsStore'

export function SessionRow({ session, active }: { session: SessionInfo; active: boolean }) {
  const loadSession = useSessionsStore((s) => s.loadSession)
  const deleteSession = useSessionsStore((s) => s.deleteSession)
  const renameSession = useSessionsStore((s) => s.renameSession)

  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(session.name)

  const commitRename = () => {
    setEditing(false)
    const trimmed = draft.trim()
    if (trimmed && trimmed !== session.name) void renameSession(session.id, trimmed)
    else setDraft(session.name)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commitRename()
    if (e.key === 'Escape') {
      setDraft(session.name)
      setEditing(false)
    }
  }

  if (editing) {
    return (
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, px: 1.5, py: 0.5 }}>
        <InputBase
          autoFocus
          fullWidth
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          onBlur={commitRename}
          sx={{ fontSize: '0.8125rem' }}
        />
        <IconButton size="small" onMouseDown={(e) => e.preventDefault()} onClick={commitRename}>
          <TickCircleLinear size={14} color="currentColor" />
        </IconButton>
      </Box>
    )
  }

  return (
    <ListItemButton
      selected={active}
      onClick={() => void loadSession(session.id)}
      sx={{
        borderRadius: 1,
        mx: 0.5,
        py: 0.5,
        '&:hover .session-row-actions': { opacity: 1 },
      }}
    >
      <Typography
        variant="body2"
        noWrap
        sx={{ flex: 1, minWidth: 0, fontWeight: active ? 600 : 400, color: active ? 'text.primary' : 'text.secondary' }}
        title={session.name}
      >
        {session.name || 'Untitled'}
      </Typography>
      <Box className="session-row-actions" sx={{ display: 'flex', opacity: 0, transition: 'opacity 0.1s', flexShrink: 0 }}>
        <IconButton
          size="small"
          onClick={(e) => {
            e.stopPropagation()
            setEditing(true)
          }}
        >
          <EditLinear size={14} color="currentColor" />
        </IconButton>
        <IconButton
          size="small"
          onClick={(e) => {
            e.stopPropagation()
            void deleteSession(session.id)
          }}
        >
          <TrashLinear size={14} color="currentColor" />
        </IconButton>
      </Box>
    </ListItemButton>
  )
}
