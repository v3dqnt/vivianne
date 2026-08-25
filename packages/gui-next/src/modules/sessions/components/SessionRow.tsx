import { memo, useState, type KeyboardEvent } from 'react'
import Box from '@mui/material/Box'
import ListItemButton from '@mui/material/ListItemButton'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import Tooltip from '@mui/material/Tooltip'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import { EditLinear, TrashLinear, TickCircleLinear, CopyLinear, FolderLinear, MessageSquareLinear } from '../../../icons'
import Divider from '@mui/material/Divider'
import type { SessionInfo } from '../lib/types'
import { useSessionsStore } from '../store/sessionsStore'
import { useProjectsStore } from '../store/projectsStore'
import { useThreadsStore } from '../store/threadsStore'
import { SESSION_DRAG_MIME } from './ChatDockZone'

// Memoized: the sidebar re-renders on every filter keystroke and sessions-
// store update; rows whose session object and active flag are unchanged bail
// out instead of diffing two MUI Menus each time.
export const SessionRow = memo(function SessionRow({ session, active }: { session: SessionInfo; active: boolean }) {
  const loadSession = useSessionsStore((s) => s.loadSession)
  const deleteSession = useSessionsStore((s) => s.deleteSession)
  const renameSession = useSessionsStore((s) => s.renameSession)
  const forkSession = useSessionsStore((s) => s.forkSession)
  const openThread = useThreadsStore((s) => s.open)

  const projects = useProjectsStore((s) => s.projects)
  const sessionProjectMap = useProjectsStore((s) => s.sessionProjectMap)
  const assignSession = useProjectsStore((s) => s.assign)
  const [projectAnchor, setProjectAnchor] = useState<HTMLElement | null>(null)
  // Right-click menu — the discoverable path to split view; dragging still
  // works but shouldn't be the only way in.
  const [contextMenu, setContextMenu] = useState<{ top: number; left: number } | null>(null)

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

  const assignedProjectId = sessionProjectMap[session.id] ?? null

  return (
    <ListItemButton
      selected={active}
      onClick={() => void loadSession(session.id)}
      // `disableRipple` matters for drag: MUI's ripple layer swallows the
      // pointer sequence the browser needs to start a native HTML5 drag, so
      // with it on the row shows a "no-drop" cursor and never fires
      // onDragStart. Setting BOTH text/plain and the custom MIME is also
      // required — some webviews (WebView2 included) refuse to initiate a
      // drag whose payload has no standard-typed data.
      disableRipple
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(SESSION_DRAG_MIME, session.id)
        e.dataTransfer.setData('text/plain', session.id)
        e.dataTransfer.effectAllowed = 'copyMove'
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        setContextMenu({ top: e.clientY, left: e.clientX })
      }}
      sx={{
        borderRadius: 1,
        mx: 0.5,
        py: 0.375,
        gap: 1,
        cursor: 'grab',
        '&:active': { cursor: 'grabbing' },
        WebkitUserDrag: 'element',
        '&:hover .session-row-actions': { opacity: 1 },
        // The active-thread marker every coding tool's session list uses —
        // a left accent bar plus a filled row, not just a faint tint (MUI's
        // default `.Mui-selected` alone reads as "barely different" against
        // this app's dark background).
        borderLeft: 2,
        borderColor: active ? 'primary.main' : 'transparent',
        '&.Mui-selected': { bgcolor: 'action.selected' },
        '&.Mui-selected:hover': { bgcolor: 'action.selected' },
      }}
    >
      <Box sx={{ display: 'flex', flexShrink: 0, color: active ? 'primary.main' : 'text.secondary' }}>
        <MessageSquareLinear size={14} color="currentColor" />
      </Box>
      <Typography
        variant="body2"
        noWrap
        sx={{ flex: 1, minWidth: 0, fontWeight: active ? 600 : 400, color: active ? 'text.primary' : 'text.secondary' }}
        title={session.name}
      >
        {session.name || 'Untitled'}
      </Typography>
      <Box className="session-row-actions" sx={{ display: 'flex', opacity: 0, transition: 'opacity 0.1s', flexShrink: 0 }}>
        <Tooltip title="Project">
          <IconButton
            size="small"
            onClick={(e) => {
              e.stopPropagation()
              setProjectAnchor(e.currentTarget)
            }}
            sx={{ color: assignedProjectId ? 'primary.main' : 'inherit' }}
          >
            <FolderLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Rename">
          <IconButton
            size="small"
            onClick={(e) => {
              e.stopPropagation()
              setEditing(true)
            }}
          >
            <EditLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Fork this session">
          <IconButton
            size="small"
            onClick={(e) => {
              e.stopPropagation()
              void forkSession(session.id)
            }}
          >
            <CopyLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Delete">
          <IconButton
            size="small"
            onClick={(e) => {
              e.stopPropagation()
              void deleteSession(session.id)
            }}
          >
            <TrashLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Box>

      {/* Menus mount only while open — a closed Menu per row used to cost a
          portal + transition group for every single session row. */}
      {projectAnchor !== null && (
        <Menu anchorEl={projectAnchor} open onClose={() => setProjectAnchor(null)} onClick={(e) => e.stopPropagation()}>
        {projects.length === 0 ? (
          <MenuItem disabled>No projects yet</MenuItem>
        ) : (
          projects.map((p) => (
            <MenuItem
              key={p.id}
              selected={p.id === assignedProjectId}
              onClick={() => {
                void assignSession(session.id, p.id)
                setProjectAnchor(null)
              }}
            >
              {p.name}
            </MenuItem>
          ))
        )}
        {assignedProjectId && (
          <MenuItem
            onClick={() => {
              void assignSession(session.id, null)
              setProjectAnchor(null)
            }}
          >
            Remove from project
          </MenuItem>
        )}
        </Menu>
      )}

      {contextMenu !== null && (
        <Menu
          open
          onClose={() => setContextMenu(null)}
          onClick={(e) => e.stopPropagation()}
          anchorReference="anchorPosition"
          anchorPosition={{ top: contextMenu.top, left: contextMenu.left }}
        >
        <MenuItem
          onClick={() => {
            openThread(session.id)
            setContextMenu(null)
          }}
        >
          Open in split view
        </MenuItem>
        <MenuItem
          onClick={() => {
            void loadSession(session.id)
            setContextMenu(null)
          }}
        >
          Open here
        </MenuItem>
        <Divider />
        <MenuItem
          onClick={() => {
            setEditing(true)
            setContextMenu(null)
          }}
        >
          Rename
        </MenuItem>
        <MenuItem
          onClick={() => {
            void forkSession(session.id)
            setContextMenu(null)
          }}
        >
          Fork
        </MenuItem>
        <MenuItem
          onClick={(e) => {
            setProjectAnchor(e.currentTarget)
            setContextMenu(null)
          }}
        >
          Project…
        </MenuItem>
        <Divider />
        <MenuItem
          onClick={() => {
            void deleteSession(session.id)
            setContextMenu(null)
          }}
          sx={{ color: 'error.main' }}
        >
          Delete
        </MenuItem>
        </Menu>
      )}
    </ListItemButton>
  )
})
