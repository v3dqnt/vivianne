import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Box, ButtonBase, IconButton, InputBase, Stack, Tooltip, Typography } from '../../../ui'
import { EditLinear, SearchNormalLinear, TrashLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { useProjectsStore, GENERAL_PROJECT_ID } from '../store/projectsStore'
import { ProjectBanner } from './ProjectBanner'
import { useDaysStore, threadsOf, GENERAL_SCOPE } from '../store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { useThreadGit } from '../../git/store/threadGitStore'
import { changedCount } from '../../git/store/gitStatusStore'
import type { SessionInfo } from '../lib/types'


/** One rhythm for every row: the same height, inset and corner. */
const ROW = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: '10px',
  width: '100%',
  minHeight: 32,
  px: '10px',
  borderRadius: '8px',
  fontSize: '0.875rem',
  textAlign: 'left',
  color: 'text.primary',
  '&:hover': { bgcolor: 'action.hover' },
  // Keyboard focus is as visible as hover, never lost on the dark card.
  '&:focus-visible': { outline: '2px solid', outlineColor: 'primary.main', outlineOffset: '-2px' },
} as const

/**
 * The threads of the project picked in the rail, and nothing of any other:
 * its name, New chat, then its threads. Each thread is its own agent.
 */
export function ThreadsSidebar() {
  const projects = useProjectsStore((s) => s.projects)
  const projectId = useProjectsStore((s) => s.activeProjectId) ?? GENERAL_PROJECT_ID
  const openId = useDaysStore((s) => s.todayId)
  const viewing = useDaysStore((s) => s.viewing)
  // Re-read when the open project's threads change (made, named, deleted).
  const localThreads = useDaysStore((s) => s.threads)
  const [sessions, setSessions] = useState<SessionInfo[] | null>(null)
  const [query, setQuery] = useState<string | null>(null)

  useEffect(() => {
    void call<SessionInfo[]>('cmd_get_sessions').then((s) => setSessions(s ?? []))
  }, [localThreads, openId, projectId])

  // A thread's title is written by a model a moment after its first message; show it as it lands.
  useEffect(() => {
    const off = on<{ id: string; name: string }>('session:renamed', ({ id, name }) => setSessions((list) => list?.map((t) => (t.id === id ? { ...t, name } : t)) ?? list))
    return () => void off.then((f) => f())
  }, [])

  const project = projects.find((p) => p.id === projectId)
  const name = projectId === GENERAL_PROJECT_ID ? 'General' : project?.name ?? 'General'
  const threads = useMemo(() => {
    if (!sessions) return null
    const list = threadsOf(projectId === GENERAL_PROJECT_ID ? GENERAL_SCOPE : projectId, sessions)
    const q = query?.trim().toLowerCase()
    return q ? list.filter((t) => titleOf(t).toLowerCase().includes(q)) : list
  }, [sessions, projectId, query])

  const open = async (threadId: string | null) => {
    const days = useDaysStore.getState()
    if (threadId) await days.openThread(threadId)
    else await days.newThread()
  }

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <ProjectBanner project={project ?? null} name={name} />

      {/* One slim toolbar: the list's label, and two icon buttons. */}
      <Stack direction="row" sx={{ alignItems: 'center', gap: '2px', height: 40, px: '14px', flexShrink: 0 }}>
        {query === null ? (
          <Typography sx={{ flex: 1, fontSize: '0.75rem', fontWeight: 500, color: 'text.secondary', letterSpacing: '0.02em' }}>Threads</Typography>
        ) : (
          <InputBase
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Escape' && setQuery(null)}
            onBlur={() => !query.trim() && setQuery(null)}
            placeholder={`Search ${name}`}
            inputProps={{ 'aria-label': `Search threads in ${name}` }}
            sx={{ flex: 1, fontSize: '0.8125rem' }}
          />
        )}
        <Tooltip title="Search threads">
          <IconButton size="small" aria-label="Search threads" onClick={() => setQuery((q) => (q === null ? '' : null))} sx={{ color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
            <SearchNormalLinear size={16} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="New chat">
          <IconButton size="small" aria-label="New chat" onClick={() => void open(null)} sx={{ color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
            <EditLinear size={16} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '10px', pb: '12px' }}>
        {threads === null && <Skeleton />}
        {threads?.length === 0 && <Empty>{query ? 'No threads match.' : 'No threads yet. Start one with the pencil above.'}</Empty>}
        {threads?.map((t) => (
          <ThreadRow key={t.id} thread={t} active={!viewing && t.id === openId} onOpen={() => void open(t.id)} />
        ))}
      </Box>
    </Stack>
  )
}

/** A thread's name as a person reads it: the app's own markers and
 * markdown stripped from a title that was cut from a message. */
export function titleOf(thread: SessionInfo): string {
  const clean = (thread.name ?? '').replace(/<[^>]*>/g, ' ').replace(/[*_`#]+/g, '').replace(/\s+/g, ' ').trim()
  return clean || 'New chat'
}

/** A thread: its name, and on the right what waits in its worktree. */
export function ThreadRow({ thread, active, onOpen }: { thread: SessionInfo; active: boolean; onOpen: () => void }) {
  const [confirming, setConfirming] = useState(false)
  const busy = useAgentStore((s) => s.isBusy)
  const folder = thread.workspace_folders?.[0]
  const git = useThreadGit((s) => (folder ? s.byPath[folder] : null))
  const ensure = useThreadGit((s) => s.ensure)
  useEffect(() => {
    if (folder) ensure([folder])
  }, [folder, ensure])
  // Threads that share a folder share its changes: the count is shown on the
  // open thread, where it is about the work on screen.
  const changed = git && active ? changedCount(git) : 0
  const title = titleOf(thread)
  return (
    <Stack direction="row" sx={{ position: 'relative', '&:hover .row-delete': { opacity: 1 }, '&:hover .row-meta': { opacity: 0 } }}>
      <ButtonBase
        onClick={onOpen}
        aria-current={active ? 'true' : undefined}
        title={git ? `${title}\n${git.branch}` : title}
        sx={{
          ...ROW,
          pr: '36px',
          bgcolor: active ? 'action.selected' : 'transparent',
          fontWeight: active ? 500 : 400,
          color: active ? 'text.primary' : 'text.secondary',
          '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover', color: 'text.primary' },
        }}
      >
        <Box component="span" sx={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {title}
        </Box>
      </ButtonBase>
      <Box className="row-meta" sx={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)', fontSize: '0.75rem', color: git?.conflicted ? 'error.main' : 'warning.main', pointerEvents: 'none', transition: 'opacity 120ms' }}>
        {changed > 0 ? `+${changed}` : ''}
      </Box>
      <Box className="row-delete" sx={{ position: 'absolute', right: 4, top: 4, opacity: confirming ? 1 : 0, transition: 'opacity 120ms' }}>
        {confirming ? (
          <Stack direction="row" sx={{ bgcolor: 'background.paper', borderRadius: '6px' }}>
            <ButtonBase
              onClick={() => {
                setConfirming(false)
                void useDaysStore.getState().deleteThread(thread.id)
              }}
              sx={{ fontSize: '0.75rem', color: 'error.main', px: '8px', height: 28, borderRadius: '6px' }}
            >
              Delete
            </ButtonBase>
            <ButtonBase onClick={() => setConfirming(false)} sx={{ fontSize: '0.75rem', color: 'text.secondary', px: '8px', height: 28, borderRadius: '6px' }}>
              Keep
            </ButtonBase>
          </Stack>
        ) : (
          <IconButton size="small" aria-label="Delete thread" disabled={busy && active} onClick={() => setConfirming(true)}>
            <TrashLinear size={14} color="currentColor" />
          </IconButton>
        )}
      </Box>
    </Stack>
  )
}

function Empty({ children, indent }: { children: ReactNode; indent?: boolean }) {
  return <Typography sx={{ fontSize: '0.875rem', color: 'text.disabled', pl: indent ? '36px' : '10px', minHeight: 32, display: 'flex', alignItems: 'center' }}>{children}</Typography>
}

/** Rows' shape while the list loads. */
function Skeleton({ indent }: { indent?: boolean }) {
  return (
    <Stack sx={{ gap: '14px', py: '10px', pl: indent ? '36px' : '10px', pr: '10px' }}>
      {[72, 58, 64].map((w) => (
        <Box key={w} sx={{ height: 10, width: `${w}%`, borderRadius: '4px', bgcolor: 'action.hover' }} />
      ))}
    </Stack>
  )
}
