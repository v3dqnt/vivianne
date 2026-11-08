import { useEffect, useMemo, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography } from '../../ui'
import { AddLinear, SearchNormalLinear } from '../../icons'
import { call } from '../../ipc'
import { useProjectsStore, GENERAL_PROJECT_ID } from '../sessions/store/projectsStore'
import { useDaysStore, threadsOf, isThreaded, GENERAL_SCOPE } from '../sessions/store/daysStore'
import { ProjectMark } from '../sessions/components/ProjectMark'
import { ProjectBanner } from '../sessions/components/ProjectBanner'
import { ThreadRow, titleOf } from '../sessions/components/ThreadsSidebar'
import type { SessionInfo } from '../sessions/lib/types'
import { useTeamStore, type TeamAgent } from '../team/store/teamStore'
import { useGeneralAgents } from '../team/store/generalAgentsStore'
import { useBotsStore, botKey } from '../team/store/botsStore'
import { NewAgentDialog } from '../team/components/NewAgentDialog'
import { Avatar, faceOf } from '../team/components/bits'
import { useBotFace } from '../team/components/AgentFace'
import { NewProjectDialog } from '../sessions/components/NewProjectDialog'
import { PairProjectDialog } from '../multiplayer/components/PairProjectDialog'
import { useAppView } from '../git/components/GitView'
import { mainPane } from '../../theme'
import { useTheme } from '../../ui'
import { useArchivedThreads } from '../../lib/archivedThreads'

const GENERAL = { id: GENERAL_PROJECT_ID, name: 'General', color: null } as const

const ROW_SX = {
  display: 'flex',
  width: '100%',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: '12px',
  height: 36,
  px: '12px',
  borderRadius: '8px',
  fontSize: '0.875rem',
  textAlign: 'left',
  '&:hover': { bgcolor: 'action.hover' },
} as const

/**
 * Where Vivianne opens: the projects on the left, and for the one picked, its
 * threads to carry on. Pick a thread, or start one, and the chat opens on it.
 */
export function HomeView() {
  const theme = useTheme()
  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const refresh = useProjectsStore((s) => s.refresh)
  const map = useProjectsStore((s) => s.sessionProjectMap)
  const localThreads = useDaysStore((s) => s.threads)
  const tabs = useDaysStore((s) => s.tabs)
  const openId = useDaysStore((s) => s.todayId)
  const [picked, setPicked] = useState<string>(activeProjectId ?? GENERAL_PROJECT_ID)
  const [sessions, setSessions] = useState<SessionInfo[] | null>(null)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)
  const [pairing, setPairing] = useState(false)
  const [tab, setTab] = useState<'projects' | 'agents'>('projects')

  useEffect(() => {
    void refresh()
  }, [refresh])
  // Re-read when threads are made, named, filed or deleted.
  useEffect(() => {
    void call<SessionInfo[]>('cmd_get_sessions').then((s) => setSessions(s ?? []))
  }, [localThreads, tabs, map])

  const project = projects.find((p) => p.id === picked) ?? null
  // General and threads projects are a list of chats; a project made before
  // threads is one conversation, and opens straight onto it.
  const scope = picked === GENERAL_PROJECT_ID ? GENERAL_SCOPE : picked
  const threaded = isThreaded(scope)
  const threads = useMemo(() => {
    if (!sessions) return null
    const list = threadsOf(scope, sessions)
    const q = query.trim().toLowerCase()
    return q ? list.filter((t) => titleOf(t).toLowerCase().includes(q)) : list
  }, [sessions, scope, query, map])

  // Archived threads are put away until asked for.
  const archivedIds = useArchivedThreads((s) => s.ids)
  const [showArchived, setShowArchived] = useState(false)
  const archivedCount = threads?.filter((t) => archivedIds.includes(t.id)).length ?? 0
  const visible = (threads ?? []).filter((t) => showArchived || !archivedIds.includes(t.id))

  const enter = async (thread: string | null) => {
    const days = useDaysStore.getState()
    if (thread) await days.focusThread(thread)
    else if (threaded) await days.newThreadIn(scope)
    else await days.openScope(scope)
    useAppView.setState({ view: 'chat' })
  }

  const all = [GENERAL, ...projects]

  return (
    <Box sx={{ flex: 1, minWidth: 0, height: '100%', bgcolor: 'background.chrome', pr: 1, pb: 1, pl: 0.5 }}>
      <Box sx={{ height: '100%', ...mainPane(theme), overflowY: 'auto' }}>
        <Stack sx={{ maxWidth: 1040, mx: 'auto', px: '32px', pt: '32px' }}>
          <Stack direction="row" role="tablist" sx={{ gap: '4px', p: '2px', borderRadius: '8px', bgcolor: 'action.hover', alignSelf: 'flex-start' }}>
            {([['projects', 'Projects'], ['agents', '24/7 agents']] as const).map(([id, label]) => (
              <ButtonBase
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => setTab(id)}
                sx={{ height: 28, px: '12px', borderRadius: '6px', fontSize: '0.8125rem', fontWeight: tab === id ? 600 : 500, color: tab === id ? 'text.primary' : 'text.secondary', bgcolor: tab === id ? 'background.paper' : 'transparent', '&:hover': { color: 'text.primary' } }}
              >
                {label}
              </ButtonBase>
            ))}
          </Stack>
        </Stack>
        {tab === 'agents' ? (
          <AlwaysOnAgents />
        ) : (
        <Stack direction="row" sx={{ maxWidth: 1040, mx: 'auto', px: '32px', py: '32px', gap: '48px', alignItems: 'flex-start', flexWrap: 'wrap' }}>
          <Stack sx={{ flex: '1 1 240px', maxWidth: 300, minWidth: 0, gap: '4px' }}>
            <Stack direction="row" sx={{ alignItems: 'center', height: 32, px: '12px', mb: '4px' }}>
              <Typography sx={{ flex: 1, fontSize: '0.8125rem', fontWeight: 500, color: 'text.secondary' }}>Projects</Typography>
              <ButtonBase onClick={() => setCreating(true)} aria-label="New project" title="New project" sx={{ width: 24, height: 24, borderRadius: '6px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
                <AddLinear size={14} color="currentColor" />
              </ButtonBase>
            </Stack>
            {all.map((p) => (
              <ButtonBase key={p.id} onClick={() => setPicked(p.id)} aria-current={picked === p.id ? 'true' : undefined} sx={{ ...ROW_SX, bgcolor: picked === p.id ? 'action.selected' : 'transparent', fontWeight: picked === p.id ? 600 : 400 }}>
                <ProjectMark project={p} />
                <Box component="span" sx={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {p.name}
                </Box>
              </ButtonBase>
            ))}
          </Stack>

          <Stack sx={{ flex: '3 1 360px', minWidth: 0, gap: '16px' }}>
            {project && (
              <Box sx={{ borderRadius: '12px', overflow: 'hidden' }}>
                <ProjectBanner project={project} name={project.name} />
              </Box>
            )}
            <Stack direction="row" sx={{ alignItems: 'center', gap: '12px' }}>
              {!project && <ProjectMark project={GENERAL} size={28} />}
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography sx={{ fontSize: '1.25rem', fontWeight: 600, lineHeight: '28px' }} noWrap>
                  {project?.name ?? 'General'}
                </Typography>
                <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }} noWrap>
                  {project?.path ?? (picked === GENERAL_PROJECT_ID ? 'Questions not tied to a project' : '')}
                </Typography>
              </Stack>
              <ButtonBase onClick={() => void enter(null)} sx={{ flexShrink: 0, whiteSpace: 'nowrap', gap: '8px', height: 32, px: '12px', borderRadius: '8px', bgcolor: 'text.primary', color: 'background.paper', fontSize: '0.8125rem', fontWeight: 500, '&:hover': { opacity: 0.85 } }}>
                <AddLinear size={14} color="currentColor" />
                {threaded ? 'New thread' : 'Open project'}
              </ButtonBase>
            </Stack>

            <>
                <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 36, px: '12px', borderRadius: '8px', bgcolor: 'action.hover', color: 'text.disabled' }}>
                  <SearchNormalLinear size={14} color="currentColor" />
                  <InputBase value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Search threads in ${project?.name ?? 'General'}`} sx={{ flex: 1, fontSize: '0.8125rem', color: 'text.primary' }} />
                </Stack>
                <Stack sx={{ gap: '2px' }}>
                  {threads === null ? (
                    <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '12px' }}>Loading…</Typography>
                  ) : visible.length === 0 ? (
                    <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '12px' }}>{query ? 'No threads match.' : 'No threads yet. Start one.'}</Typography>
                  ) : (
                    visible.map((t) => <ThreadRow key={t.id} thread={t} active={t.id === openId} onOpen={() => void enter(t.id)} />)
                  )}
                </Stack>
                {archivedCount > 0 && (
                  <ButtonBase onClick={() => setShowArchived((v) => !v)} sx={{ alignSelf: 'flex-start', px: '12px', height: 28, borderRadius: '6px', fontSize: '0.75rem', color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
                    {showArchived ? 'Hide archived' : `Show archived (${archivedCount})`}
                  </ButtonBase>
                )}
              </>
          </Stack>
        </Stack>
        )}
      </Box>
      <NewProjectDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(p) => setPicked(p.id)}
        onMultiplayer={() => {
          setCreating(false)
          setPairing(true)
        }}
      />
      <PairProjectDialog open={pairing} onClose={() => setPairing(false)} />
    </Box>
  )
}

const NO_AGENTS: TeamAgent[] = []

/** General's own agents: always on, each with its job, schedule, inbox and
 * phone. Pick one to talk to it. */
function AlwaysOnAgents() {
  const root = useGeneralAgents((s) => s.root)
  const ensure = useGeneralAgents((s) => s.ensure)
  const agents = useTeamStore((s) => (root ? (s.byRoot[root] ?? NO_AGENTS) : NO_AGENTS))
  const statuses = useBotsStore((s) => s.status)
  const loadStatus = useBotsStore((s) => s.loadStatus)
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    void ensure().then((r) => r && void loadStatus(r))
  }, [ensure, loadStatus])

  const open = (id: string) => {
    if (!root) return
    useBotsStore.getState().setChannel(root, id)
    useAppView.setState({ view: 'agents' })
  }
  const active = agents.filter((a) => a.status === 'active')

  return (
    <Stack sx={{ maxWidth: 1040, mx: 'auto', px: '32px', py: '32px', gap: '16px' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '16px' }}>
        <Typography sx={{ flex: 1, fontSize: '0.8125rem', color: 'text.secondary' }}>
          Agents that keep working on their own: on a schedule, when something arrives, or when you message them.
        </Typography>
        <ButtonBase
          onClick={() => setCreating(true)}
          disabled={!root}
          sx={{ flexShrink: 0, whiteSpace: 'nowrap', gap: '8px', height: 32, px: '12px', borderRadius: '8px', bgcolor: 'text.primary', color: 'background.paper', fontSize: '0.8125rem', fontWeight: 500, '&:hover': { opacity: 0.85 } }}
        >
          <AddLinear size={14} color="currentColor" />
          New agent
        </ButtonBase>
      </Stack>
      {root && <NewAgentDialog root={root} open={creating} count={agents.length} onClose={() => setCreating(false)} />}
      {active.length === 0 ? (
        <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>No 24/7 agents yet.</Typography>
      ) : (
        <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: '12px' }}>
          {active.map((a) => {
            const st = root ? statuses[botKey(root, a.id)] : undefined
            const working = st?.state === 'working'
            const paused = st?.state === 'paused'
            return (
              <ButtonBase
                key={a.id}
                onClick={() => open(a.id)}
                sx={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: '8px', p: '16px', borderRadius: '12px', border: 1, borderColor: 'divider', textAlign: 'left', '&:hover': { bgcolor: 'action.hover' } }}
              >
                <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', width: '100%' }}>
                  <CardFace root={root} agent={a} />
                  <Box component="span" sx={{ flex: 1, minWidth: 0, fontSize: '0.875rem', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {a.name}
                  </Box>
                  <Stack direction="row" sx={{ alignItems: 'center', gap: '4px', flexShrink: 0 }}>
                    <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: working ? 'success.main' : paused ? 'warning.main' : 'text.disabled' }} />
                    <Box component="span" sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{working ? 'Working' : paused ? 'Paused' : 'Idle'}</Box>
                  </Stack>
                </Stack>
                <Box component="span" sx={{ fontSize: '0.75rem', color: 'text.secondary', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                  {working && st?.activity ? st.activity : a.role}
                </Box>
              </ButtonBase>
            )
          })}
        </Box>
      )}
    </Stack>
  )
}

/** A card's avatar, live: it shows the agent working, thinking or idle. */
function CardFace({ root, agent }: { root: string | null; agent: TeamAgent }) {
  const face = useBotFace(root, agent.id)
  return <Avatar {...faceOf(agent)} size={36} state={face} animate />
}
