import { useEffect, useMemo, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import ButtonBase from '@mui/material/ButtonBase'
import Tooltip from '@mui/material/Tooltip'
import { MessageAddLinear, SearchNormalLinear, CloseCircleLinear, ExportLinear } from '../../../icons'
import { useSessionsStore } from '../store/sessionsStore'
import { useProjectsStore, projectColor } from '../store/projectsStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { SessionRow } from './SessionRow'

export const SIDEBAR_WIDTH = 260

/** Thread list for the active project.
 *
 * Threads always belong to a project now — the workspace/project grouping
 * toggle this used to carry is gone, since the project rail already answers
 * "which group am I looking at". A new thread is assigned to the active
 * project on creation, so nothing can end up orphaned. */
export function Sidebar() {
  const sessions = useSessionsStore((s) => s.sessions)
  const activeSessionId = useSessionsStore((s) => s.activeSessionId)
  const refresh = useSessionsStore((s) => s.refresh)
  const refreshActiveId = useSessionsStore((s) => s.refreshActiveId)
  const newChat = useSessionsStore((s) => s.newChat)
  const isBusy = useAgentStore((s) => s.isBusy)
  const exportMarkdown = useSessionsStore((s) => s.exportMarkdown)

  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const sessionProjectMap = useProjectsStore((s) => s.sessionProjectMap)
  const assign = useProjectsStore((s) => s.assign)
  const refreshProjects = useProjectsStore((s) => s.refresh)

  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const searchRef = useRef<HTMLInputElement>(null)
  /** Serializes New-session clicks: `startChat` reads `activeSessionId`
   * after an await, so overlapping runs would each claim a different id and
   * assign the wrong ones to the project. */
  const startingRef = useRef(false)

  useEffect(() => {
    void refresh()
    void refreshActiveId()
    void refreshProjects()
  }, [refresh, refreshActiveId, refreshProjects])

  // A turn just finished (auto-saved server-side on every turn) — the
  // sidebar's list is otherwise stale until this, most visibly for a brand
  // new thread that didn't exist as a session until its first reply landed.
  const wasBusy = useRef(false)
  useEffect(() => {
    if (wasBusy.current && !isBusy) {
      void (async () => {
        await refresh()
        // A thread has no server-side id until its first turn completes, so
        // this is the earliest point a new chat can be claimed by the
        // project it was started in. Without it the thread stays unassigned
        // and — since this list filters by project — never appears at all,
        // which is what made "New session" look like it did nothing.
        const id = useSessionsStore.getState().activeSessionId
        const assigned = useProjectsStore.getState().sessionProjectMap
        if (id && activeProjectId && !assigned[id]) await assign(id, activeProjectId)
      })()
    }
    wasBusy.current = isBusy
  }, [isBusy, refresh, activeProjectId, assign])

  useEffect(() => {
    if (searchOpen) searchRef.current?.focus()
  }, [searchOpen])

  const activeProject = projects.find((p) => p.id === activeProjectId) ?? null

  const visible = useMemo(() => {
    const mine = sessions.filter((s) => sessionProjectMap[s.id] === activeProjectId)
    const q = query.trim().toLowerCase()
    return q ? mine.filter((s) => s.name.toLowerCase().includes(q)) : mine
  }, [sessions, sessionProjectMap, activeProjectId, query])

  const startChat = async () => {
    if (startingRef.current) return
    startingRef.current = true
    try {
      await startChatInner()
    } finally {
      startingRef.current = false
    }
  }

  const startChatInner = async () => {
    await newChat()
    // The thread is persisted up front now, so its id exists here and it can
    // be claimed for this project straight away — no waiting for the first
    // turn to finish.
    const id = useSessionsStore.getState().activeSessionId
    if (id && activeProjectId) await assign(id, activeProjectId)
    // Creating a thread resets the backend workspace, so point it back at
    // this project's folder — otherwise the agent starts with no workspace
    // and can't see the project at all.
    if (activeProject?.path) {
      const ws = useWorkspaceStore.getState()
      await ws.setWorkspace([activeProject.path], ws.accessMode)
    }
  }

  const exportActive = () => {
    const md = exportMarkdown()
    void navigator.clipboard.writeText(md).catch(() => {
      // clipboard can be rejected (no permission in WebView2) — nothing to
      // show beyond the export dialog's copy button, so fail quietly
    })
  }

  if (!activeProject) {
    return (
      <Stack component="nav" sx={{ width: SIDEBAR_WIDTH, flexShrink: 0, height: '100%', borderRight: 1, borderColor: 'divider', p: 2, gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          No projects yet
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Create one with the + button on the left to start a thread.
        </Typography>
      </Stack>
    )
  }

  return (
    <Stack
      component="nav"
      sx={{ width: SIDEBAR_WIDTH, flexShrink: 0, height: '100%', borderRight: 1, borderColor: 'divider', bgcolor: 'background.default' }}
    >
      <Stack sx={{ px: 2, pt: 2, pb: 1.5, gap: 0.25, flexShrink: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0 }}>
          <Box sx={{ width: 8, height: 8, borderRadius: '50%', flexShrink: 0, bgcolor: projectColor(activeProject) }} />
          <Typography variant="subtitle2" sx={{ fontWeight: 700, minWidth: 0 }} noWrap title={activeProject.name}>
            {activeProject.name}
          </Typography>
        </Stack>
        <Typography variant="caption" color="text.secondary" noWrap title={activeProject.path ?? undefined}>
          {activeProject.path ?? 'No folder set'}
        </Typography>
      </Stack>

      <Box sx={{ px: 1.5, pb: 1, flexShrink: 0 }}>
        <ButtonBase
          onClick={() => void startChat()}
          sx={{
            display: 'flex',
            width: '100%',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 1,
            py: 1,
            borderRadius: 1.5,
            border: 1,
            borderColor: 'divider',
            fontSize: '0.8125rem',
            fontWeight: 600,
            transition: 'background-color 0.15s, border-color 0.15s, transform 0.08s',
            '&:hover': { bgcolor: 'action.hover', borderColor: 'text.secondary' },
            '&:active': { transform: 'scale(0.98)' },
          }}
        >
          <MessageAddLinear size={15} color="currentColor" />
          New session
        </ButtonBase>
      </Box>

      <Stack direction="row" sx={{ alignItems: 'center', px: 1.5, pb: 0.75, gap: 0.25, flexShrink: 0 }}>
        {searchOpen ? (
          <Stack direction="row" sx={{ alignItems: 'center', flex: 1, gap: 0.75 }}>
            <SearchNormalLinear size={14} color="currentColor" />
            <InputBase
              inputRef={searchRef}
              fullWidth
              placeholder="Search threads…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setQuery('')
                  setSearchOpen(false)
                }
              }}
              sx={{ fontSize: '0.8125rem' }}
            />
            <IconButton
              size="small"
              onClick={() => {
                setQuery('')
                setSearchOpen(false)
              }}
            >
              <CloseCircleLinear size={14} color="currentColor" />
            </IconButton>
          </Stack>
        ) : (
          <>
            <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.625rem' }}>
              Threads
            </Typography>
            <Box sx={{ flex: 1 }} />
            <Tooltip title="Search threads">
              <IconButton size="small" onClick={() => setSearchOpen(true)}>
                <SearchNormalLinear size={14} color="currentColor" />
              </IconButton>
            </Tooltip>
            {activeSessionId && (
              <Tooltip title="Export conversation (markdown)">
                <IconButton size="small" onClick={exportActive}>
                  <ExportLinear size={13} color="currentColor" />
                </IconButton>
              </Tooltip>
            )}
          </>
        )}
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', pb: 1 }}>
        {visible.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 2, py: 1.5 }}>
            {query ? 'No matching threads.' : 'No threads in this project yet.'}
          </Typography>
        ) : (
          visible.map((session) => <SessionRow key={session.id} session={session} active={session.id === activeSessionId} />)
        )}
      </Box>
    </Stack>
  )
}
