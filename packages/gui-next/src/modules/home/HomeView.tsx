import { useEffect, useMemo, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography } from '../../ui'
import { AddLinear, MessageSquareLinear, SearchNormalLinear } from '../../icons'
import { call } from '../../ipc'
import { useProjectsStore, projectColor, projectInitial, modeOf, GENERAL_PROJECT_ID, type Project } from '../sessions/store/projectsStore'
import { useDaysStore, threadsOf, GENERAL_SCOPE } from '../sessions/store/daysStore'
import type { SessionInfo } from '../sessions/lib/types'
import { useTeamStore, type TeamAgent } from '../team/store/teamStore'
import { useGeneralAgents } from '../team/store/generalAgentsStore'
import { useBotsStore, botKey } from '../team/store/botsStore'
import { NewAgentDialog } from '../team/components/NewAgentDialog'
import { Avatar, faceOf } from '../team/components/bits'
import { useBotFace } from '../team/components/AgentFace'
import type { BotMessage } from '../team/store/botsStore'
import { NewProjectDialog } from '../sessions/components/NewProjectDialog'
import { PairProjectDialog } from '../multiplayer/components/PairProjectDialog'
import { useAppView } from '../git/components/GitView'
import { mainPane } from '../../theme'
import { useTheme } from '../../ui'

const GENERAL = { id: GENERAL_PROJECT_ID, name: 'General', color: null } as const

function when(iso: string | number): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  const days = Math.floor((Date.now() - d.getTime()) / 86_400_000)
  if (days < 1) return d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  if (days < 7) return d.toLocaleDateString(undefined, { weekday: 'short' })
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** A project's square: its picture, or its initial on its colour. */
function ProjectMark({ project, size = 20 }: { project: Pick<Project, 'id' | 'name' | 'color'> & { icon?: string | null }; size?: number }) {
  const color = projectColor(project)
  if (project.id === GENERAL_PROJECT_ID) {
    return (
      <Box sx={{ width: size, height: size, borderRadius: '6px', display: 'grid', placeItems: 'center', bgcolor: 'action.selected', color: 'text.secondary', flexShrink: 0 }}>
        <MessageSquareLinear size={size * 0.6} color="currentColor" />
      </Box>
    )
  }
  if (project.icon) return <Box component="img" src={project.icon} alt="" sx={{ width: size, height: size, borderRadius: '6px', objectFit: 'cover', flexShrink: 0 }} />
  return (
    <Box sx={{ width: size, height: size, borderRadius: '6px', display: 'grid', placeItems: 'center', bgcolor: `${color}26`, color, fontSize: size * 0.5, fontWeight: 700, flexShrink: 0 }}>
      {projectInitial(project.name)}
    </Box>
  )
}

const ROW_SX = {
  display: 'flex',
  width: '100%',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: '12px',
  height: 36,
  px: '12px',
  borderRadius: '8px',
  fontSize: '14px',
  textAlign: 'left',
  '&:hover': { bgcolor: 'action.hover' },
} as const

/**
 * Where Vivianne opens: the projects on the left, and for the one picked, its
 * threads to carry on (or, for a team project, what the team did last). Pick
 * a thread, or start one, and the chat opens on it.
 */
export function HomeView() {
  const theme = useTheme()
  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const refresh = useProjectsStore((s) => s.refresh)
  const [picked, setPicked] = useState<string>(activeProjectId ?? GENERAL_PROJECT_ID)
  const [sessions, setSessions] = useState<SessionInfo[] | null>(null)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)
  const [pairing, setPairing] = useState(false)
  const [tab, setTab] = useState<'projects' | 'agents'>('projects')

  useEffect(() => {
    void refresh()
    void call<SessionInfo[]>('cmd_get_sessions').then((s) => setSessions(s ?? []))
  }, [refresh])

  const project = projects.find((p) => p.id === picked) ?? null
  const isTeam = !!project && modeOf(project) === 'tree'
  const threads = useMemo(() => {
    if (!sessions) return null
    const list = threadsOf(picked === GENERAL_PROJECT_ID ? GENERAL_SCOPE : picked, sessions)
    const q = query.trim().toLowerCase()
    return q ? list.filter((t) => (t.name || 'New thread').toLowerCase().includes(q)) : list
  }, [sessions, picked, query])

  const enter = async (thread: string | null) => {
    if (useProjectsStore.getState().activeProjectId !== picked) useProjectsStore.getState().setActiveProject(picked)
    const days = useDaysStore.getState()
    if (thread) await days.openThread(thread)
    else if (!isTeam) await days.newThread()
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
                sx={{ height: 28, px: '12px', borderRadius: '6px', fontSize: '13px', fontWeight: tab === id ? 600 : 500, color: tab === id ? 'text.primary' : 'text.secondary', bgcolor: tab === id ? 'background.paper' : 'transparent', '&:hover': { color: 'text.primary' } }}
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
              <Typography sx={{ flex: 1, fontSize: '13px', fontWeight: 600, color: 'text.secondary' }}>Projects</Typography>
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
                {'mode' in p && modeOf(p) === 'tree' && <Box component="span" sx={{ fontSize: '11px', color: 'text.disabled' }}>Team</Box>}
              </ButtonBase>
            ))}
          </Stack>

          <Stack sx={{ flex: '3 1 360px', minWidth: 0, gap: '16px' }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: '12px' }}>
              <ProjectMark project={project ?? GENERAL} size={28} />
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography sx={{ fontSize: '20px', fontWeight: 600, lineHeight: '28px' }} noWrap>
                  {project?.name ?? 'General'}
                </Typography>
                <Typography sx={{ fontSize: '12px', color: 'text.secondary' }} noWrap>
                  {project?.path ?? (picked === GENERAL_PROJECT_ID ? 'Questions not tied to a project' : '')}
                </Typography>
              </Stack>
              <ButtonBase onClick={() => void enter(null)} sx={{ flexShrink: 0, whiteSpace: 'nowrap', gap: '8px', height: 32, px: '12px', borderRadius: '8px', bgcolor: 'text.primary', color: 'background.paper', fontSize: '13px', fontWeight: 600, '&:hover': { opacity: 0.85 } }}>
                <AddLinear size={14} color="currentColor" />
                {isTeam ? 'Open the team' : 'New thread'}
              </ButtonBase>
            </Stack>

            {isTeam && project?.path ? (
              <TeamActivity root={project.path} onOpen={() => void enter(null)} />
            ) : (
              <>
                <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 36, px: '12px', borderRadius: '8px', bgcolor: 'action.hover', color: 'text.disabled' }}>
                  <SearchNormalLinear size={14} color="currentColor" />
                  <InputBase value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Search threads in ${project?.name ?? 'General'}`} sx={{ flex: 1, fontSize: '13px', color: 'text.primary' }} />
                </Stack>
                <Stack sx={{ gap: '2px' }}>
                  {threads === null ? (
                    <Typography sx={{ fontSize: '13px', color: 'text.secondary', px: '12px' }}>Loading…</Typography>
                  ) : threads.length === 0 ? (
                    <Typography sx={{ fontSize: '13px', color: 'text.secondary', px: '12px' }}>{query ? 'No threads match.' : 'No threads yet. Start one.'}</Typography>
                  ) : (
                    threads.map((t) => (
                      <ButtonBase key={t.id} onClick={() => void enter(t.id)} sx={ROW_SX}>
                        <Box component="span" sx={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {t.name || 'New thread'}
                        </Box>
                        <Box component="span" sx={{ fontSize: '12px', color: 'text.disabled', flexShrink: 0 }}>
                          {when(t.created_at)}
                        </Box>
                      </ButtonBase>
                    ))
                  )}
                </Stack>
              </>
            )}
          </Stack>
        </Stack>
        )}
      </Box>
      <NewProjectDialog
        open={creating}
        onClose={() => setCreating(false)}
        onMultiplayer={() => {
          setCreating(false)
          setPairing(true)
        }}
      />
      <PairProjectDialog open={pairing} onClose={() => setPairing(false)} />
    </Box>
  )
}

/** A team project: who is on it and the last things said in its room. */
function TeamActivity({ root, onOpen }: { root: string; onOpen: () => void }) {
  const team = useTeamStore((s) => s.byRoot[root])
  const load = useTeamStore((s) => s.load)
  const [room, setRoom] = useState<BotMessage[] | null>(null)

  useEffect(() => {
    void load(root)
    void call<BotMessage[]>('cmd_bots_room', { root })
      .then((m) => setRoom(m ?? []))
      .catch(() => setRoom([]))
  }, [root, load])

  const name = (from: string) => (from === 'user' ? 'You' : from === 'system' ? 'Vivianne' : (team?.find((a) => a.id === from)?.name ?? from))
  const recent = (room ?? []).slice(-12).reverse()
  const active = (team ?? []).filter((a) => a.status === 'active')

  return (
    <Stack sx={{ gap: '16px' }}>
      {active.length > 0 && (
        <Stack direction="row" sx={{ gap: '8px', flexWrap: 'wrap' }}>
          {active.map((a) => (
            <Box key={a.id} sx={{ height: 24, px: '8px', borderRadius: '6px', border: 1, borderColor: 'divider', fontSize: '12px', display: 'inline-flex', alignItems: 'center', color: 'text.secondary' }}>
              {a.name}
            </Box>
          ))}
        </Stack>
      )}
      <Typography sx={{ fontSize: '13px', fontWeight: 600, color: 'text.secondary', px: '12px' }}>Latest from the team</Typography>
      <Stack sx={{ gap: '2px' }}>
        {room === null ? (
          <Typography sx={{ fontSize: '13px', color: 'text.secondary', px: '12px' }}>Loading…</Typography>
        ) : recent.length === 0 ? (
          <Typography sx={{ fontSize: '13px', color: 'text.secondary', px: '12px' }}>Nothing yet. Open the team and give it something to do.</Typography>
        ) : (
          recent.map((m) => (
            <ButtonBase key={m.id} onClick={onOpen} sx={{ ...ROW_SX, height: 'auto', minHeight: 36, py: '8px', alignItems: 'flex-start' }}>
              <Box component="span" sx={{ width: 120, flexShrink: 0, fontWeight: 600, fontSize: '13px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {name(m.from)}
              </Box>
              <Box component="span" sx={{ flex: 1, minWidth: 0, fontSize: '13px', color: 'text.secondary', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                {m.text}
              </Box>
              <Box component="span" sx={{ fontSize: '12px', color: 'text.disabled', flexShrink: 0 }}>
                {when(m.ts)}
              </Box>
            </ButtonBase>
          ))
        )}
      </Stack>
    </Stack>
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
        <Typography sx={{ flex: 1, fontSize: '13px', color: 'text.secondary' }}>
          Agents that keep working on their own: on a schedule, when something arrives, or when you message them.
        </Typography>
        <ButtonBase
          onClick={() => setCreating(true)}
          disabled={!root}
          sx={{ flexShrink: 0, whiteSpace: 'nowrap', gap: '8px', height: 32, px: '12px', borderRadius: '8px', bgcolor: 'text.primary', color: 'background.paper', fontSize: '13px', fontWeight: 600, '&:hover': { opacity: 0.85 } }}
        >
          <AddLinear size={14} color="currentColor" />
          New agent
        </ButtonBase>
      </Stack>
      {root && <NewAgentDialog root={root} open={creating} count={agents.length} onClose={() => setCreating(false)} />}
      {active.length === 0 ? (
        <Typography sx={{ fontSize: '13px', color: 'text.secondary' }}>No 24/7 agents yet.</Typography>
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
                  <Box component="span" sx={{ flex: 1, minWidth: 0, fontSize: '14px', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {a.name}
                  </Box>
                  <Stack direction="row" sx={{ alignItems: 'center', gap: '4px', flexShrink: 0 }}>
                    <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: working ? 'success.main' : paused ? 'warning.main' : 'text.disabled' }} />
                    <Box component="span" sx={{ fontSize: '11px', color: 'text.disabled' }}>{working ? 'Working' : paused ? 'Paused' : 'Idle'}</Box>
                  </Stack>
                </Stack>
                <Box component="span" sx={{ fontSize: '12px', color: 'text.secondary', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
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
