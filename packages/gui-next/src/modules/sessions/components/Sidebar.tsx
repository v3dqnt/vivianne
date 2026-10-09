import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, IconButton, InputBase, ButtonBase, Tooltip, Collapse, alpha, useTheme } from '../../../ui'
import { ExportLinear, AddLinear, TrashLinear } from '../../../icons'
import { useProjectsStore, projectColor, modeOf, GENERAL_PROJECT_ID } from '../store/projectsStore'
import { useDaysStore } from '../store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { contentToText, type AgentMessage } from '../../ai/lib/types'
import { isAppText } from '../../ai/components/HandoffDivider'
import { surfaceCard } from '../../../theme'
import { pathSeparator } from '../../../lib/platform'
import type { SessionInfo } from '../lib/types'
import { useMasterStore } from '../store/masterStore'
import { CharterDialog } from './CharterDialog'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { BranchIndicator } from '../../ai/components/BranchIndicator'
import { OnboardingDialog } from './OnboardingDialog'
import { useTeamStore, isWorking, type TeamAgent, type AgentRun } from '../../team/store/teamStore'
import { Avatar, colorOf } from '../../team/components/bits'
import { openPanel } from '../../workspace/layout'
import { TeamConversations } from '../../team/components/TeamConversations'
import { CompanyTree } from '../../team/components/CompanyTree'
import { GeneralAgents } from '../../team/components/GeneralAgents'
import { useGeneralAgents } from '../../team/store/generalAgentsStore'
import { useBotsStore } from '../../team/store/botsStore'

export const SIDEBAR_WIDTH = 260

/** Last two path segments, e.g. `Project Versa\viv-main-gui-exp`.
 *
 * A full Windows path overflows the sidebar and gets clipped from the RIGHT,
 * which removes the only part that identifies the folder. The tail is what
 * the reader needs; the whole path stays available on hover. */
function shortPath(path: string | null | undefined): string {
  if (!path) return 'No folder set'
  const parts = path.split(/[\\/]/).filter(Boolean)
  if (parts.length <= 2) return path
  return parts.slice(-2).join(pathSeparator)
}

function dateOf(day: string): Date {
  const [y, m, d] = day.split('-').map(Number)
  return new Date(y, (m ?? 1) - 1, d ?? 1)
}

/** Month heading for grouping history, e.g. "September 2026". */
function monthOf(dateLike: string): string {
  const date = dateLike.length === 10 ? dateOf(dateLike) : new Date(dateLike)
  return date.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
}

function toMarkdown(title: string, messages: AgentMessage[]): string {
  const lines: string[] = [`# ${title}`, '']
  for (const m of messages) {
    const text = contentToText(m.content)
    if (m.type === 'user') {
      if (isAppText(text)) continue
      lines.push('## You', '', text, '')
    } else if (m.type === 'assistant') {
      lines.push('## Vivianne', '', text, '')
    } else if (m.type === 'toolResult') {
      lines.push(`_tool: ${m.toolName}${m.isError ? ' (error)' : ''}_`, text ? `\n\`\`\`\n${text}\n\`\`\`` : '', '')
    }
  }
  return lines.join('\n')
}

const SECTION_LABEL_SX = {
  fontWeight: 600,
  letterSpacing: 0.4,
  textTransform: 'uppercase',
  fontSize: '0.6875rem',
} as const

/**
 * The active project: its one conversation (with the master agent), the team
 * the master directs, and the archive of threads from before.
 *
 * There is no "New session" and no list of days. A project has a single
 * conversation for its whole life; the specialists' work is watched from
 * their own pages, not chatted with.
 */
export function Sidebar() {
  const theme = useTheme()
  const SIDEBAR_CARD_SX = {
    width: SIDEBAR_WIDTH,
    flexShrink: 0,
    height: 'auto',
    // Terax gutters: horizontal only — 8px at the window edge, a narrow gap
    // to the next pane — and flush with the title bar above.
    mt: 0,
    mb: 1,
    ml: 1,
    mr: 0.25,
    overflow: 'hidden',
    ...surfaceCard(theme.palette.mode),
  } as const

  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const sessionProjectMap = useProjectsStore((s) => s.sessionProjectMap)
  const refreshProjects = useProjectsStore((s) => s.refresh)

  const archive = useDaysStore((s) => s.archive)
  const viewing = useDaysStore((s) => s.viewing)
  const view = useDaysStore((s) => s.view)
  const backToToday = useDaysStore((s) => s.backToToday)
  const refreshArchive = useDaysStore((s) => s.refreshArchive)

  const [archiveOpen, setArchiveOpen] = useState(false)
  const liveMessages = useAgentStore((s) => s.messages)
  const earlier = useDaysStore((s) => s.earlier)

  useEffect(() => {
    void refreshProjects()
    void refreshArchive()
  }, [refreshProjects, refreshArchive])

  const isGeneral = activeProjectId === GENERAL_PROJECT_ID
  const activeProject = projects.find((p) => p.id === activeProjectId) ?? null
  const mode = modeOf(activeProject)
  const threaded = isGeneral || mode === 'threads'

  const root = activeProject?.path ?? null
  const master = useMasterStore((s) => (root ? s.byRoot[root] : null))
  const loadMaster = useMasterStore((s) => s.load)
  const team = useTeamStore((s) => (root ? s.byRoot[root] : undefined))
  const loadTeam = useTeamStore((s) => s.load)
  useEffect(() => {
    if (root) void loadTeam(root)
  }, [root, loadTeam])
  const pendingHires = team?.filter((a) => a.status === 'proposed').length ?? 0
  const activeTeam = useMemo(() => team?.filter((a) => a.status === 'active') ?? [], [team])
  const runs = useTeamStore((s) => s.runs)
  const focusAgent = useTeamStore((s) => s.focus)
  const openAgent = (id: string | null) => {
    if (!root) return
    focusAgent(root, id)
    openPanel('agents')
  }
  const [charterOpen, setCharterOpen] = useState(false)
  const [briefOpen, setBriefOpen] = useState(false)

  // Re-read on every project switch rather than caching forever: the master
  // edits the charter itself, and the dialog should show what's on disk now.
  useEffect(() => {
    if (root) void loadMaster(root)
  }, [root, loadMaster])

  // The archive belongs to projects the same way threads did: by the
  // assignment map. General gets the ones never assigned to anything.
  const myArchive = useMemo(() => {
    const mine = archive.filter((s) =>
      isGeneral ? !sessionProjectMap[s.id] : sessionProjectMap[s.id] === activeProjectId,
    )
    return mine
  }, [archive, sessionProjectMap, activeProjectId, isGeneral])

  // The last thing the user asked, as the conversation row's preview.
  const lastAsked = useMemo(() => {
    for (const list of [liveMessages, earlier]) {
      for (let i = list.length - 1; i >= 0; i--) {
        const m = list[i]
        if (m.type !== 'user') continue
        const text = contentToText(m.content).trim()
        if (text && !isAppText(text)) return text
      }
    }
    return ''
  }, [liveMessages, earlier])

  const exportCurrent = () => {
    const title = viewing ? viewing.label : `${activeProject?.name ?? 'General'} — conversation`
    const messages = viewing ? viewing.messages : [...useDaysStore.getState().earlier, ...useAgentStore.getState().messages]
    void navigator.clipboard.writeText(toMarkdown(title, messages)).catch(() => {
      // WebView2 can refuse clipboard access; nothing useful to show.
    })
  }

  if (!activeProject && !isGeneral) {
    return (
      <Stack component="nav" sx={{ ...SIDEBAR_CARD_SX, p: 2, gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          No projects yet
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Create one with the + button on the left.
        </Typography>
      </Stack>
    )
  }

  const onToday = !viewing
  // A company project with its lead: the lead, its employees and their
  // subagents, as one tree.
  const companyTree = mode === 'company'

  return (
    <Stack component="nav" sx={SIDEBAR_CARD_SX}>
      <Stack sx={{ px: 2, pt: 2, pb: 1.5, gap: 0.5, flexShrink: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, minWidth: 0 }}>
          {activeProject && (
            <Box sx={{ width: 6, height: 18, borderRadius: '3px', flexShrink: 0, bgcolor: projectColor(activeProject) }} />
          )}
          <Typography
            variant="subtitle2"
            sx={{ fontWeight: 700, minWidth: 0, letterSpacing: '-0.01em' }}
            noWrap
            title={activeProject?.name ?? 'General'}
          >
            {activeProject?.name ?? 'General'}
          </Typography>
        </Stack>
        <Typography
          variant="caption"
          color="text.secondary"
          noWrap
          title={activeProject?.path ?? undefined}
          sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem' }}
        >
          {isGeneral ? 'Not tied to a project' : shortPath(activeProject?.path)}
        </Typography>
      </Stack>

      {/* Where the agent works and on which branch — properties of the
          project, so they live with the project rather than in the composer. */}
      <Stack direction="row" sx={{ px: 1.25, pb: 1.25, gap: 0.5, flexWrap: 'wrap', flexShrink: 0 }}>
        <WorkspaceSelector />
        <BranchIndicator />
      </Stack>

      {activeProject?.path && mode === 'company' && master !== undefined && (
        <Box sx={{ px: 1.5, pb: 1.25, flexShrink: 0 }}>
          {master ? null : (
            // Every project made before onboarding existed lands here. Offered,
            // not forced: a project can go on without a master, as a plain chat.
            <Stack
              sx={{
                gap: 0.75,
                p: 1.25,
                borderRadius: '10px',
                border: 1,
                borderColor: 'divider',
                borderStyle: 'dashed',
              }}
            >
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
                No master agent yet. Describe this project once and it works from that brief every day.
              </Typography>
              <ButtonBase
                onClick={() => setBriefOpen(true)}
                sx={{
                  alignSelf: 'flex-start',
                  px: 1.25,
                  py: 0.5,
                  borderRadius: '8px',
                  fontSize: '0.75rem',
                  fontWeight: 600,
                  color: 'primary.main',
                  bgcolor: alpha(theme.palette.primary.main, 0.1),
                  '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.18) },
                }}
              >
                Brief a master agent
              </ButtonBase>
            </Stack>
          )}
        </Box>
      )}

      {threaded ? (
        <>
          {isGeneral && <GeneralAgents labelSx={SECTION_LABEL_SX} />}
          <GeneralThreads onExport={exportCurrent} />
        </>
      ) : (
      <>
      {companyTree && master && root && activeProject ? (
        <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.5, pb: 1 }}>
          <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
            <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
              Company{activeTeam.length ? ` · ${activeTeam.length}` : ''}
            </Typography>
            <Tooltip title={viewing ? 'Copy this thread as markdown' : 'Copy the whole conversation as markdown'}>
              <IconButton size="small" aria-label="Copy as markdown" onClick={exportCurrent}>
                <ExportLinear size={13} color="currentColor" />
              </IconButton>
            </Tooltip>
          </Stack>
          <CompanyTree
            leadName={master.name}
            leadColor={projectColor(activeProject)}
            leadActive={onToday}
            team={activeTeam}
            runs={(id) => runs[`${root}::${id}`]}
            pendingHires={pendingHires}
            onLead={() => {
              backToToday()
              // Straight to typing: say the goal, the lead routes it.
              window.dispatchEvent(new Event('viv:focus-composer'))
            }}
            onCharter={() => {
              // The master may have edited the charter since it was loaded.
              void loadMaster(root)
              setCharterOpen(true)
            }}
            onAgent={openAgent}
            onManage={() => openAgent(null)}
          />
        </Box>
      ) : (
      <>
      {!(mode === 'team' && root) && (
      <Box sx={{ px: 1.5, pb: 1.25, flexShrink: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
            Conversation
          </Typography>
          <Tooltip title={viewing ? 'Copy this thread as markdown' : 'Copy the whole conversation as markdown'}>
            <IconButton size="small" aria-label="Copy as markdown" onClick={exportCurrent}>
              <ExportLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
        <DayRow
          primary
          active={onToday}
          label={mode === 'team' ? 'With your team' : master ? `With ${master.name}` : 'Vivianne'}
          preview={lastAsked || 'Nothing asked yet'}
          onClick={backToToday}
        />
      </Box>
      )}

      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', pb: 1 }}>
        {/* An agent team talks in a group chat and in DMs with each teammate. */}
        {root && mode === 'team' ? (
          <TeamConversations root={root} team={activeTeam} onManage={() => openAgent(null)} />
        ) : root && (
          <Box sx={{ px: 1.5 }}>
            <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
              <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
                Team{activeTeam.length ? ` · ${activeTeam.length}` : ''}
              </Typography>
              <Tooltip title="Create or manage agents">
                <IconButton size="small" aria-label="Manage agents" onClick={() => openAgent(null)}>
                  <AddLinear size={13} color="currentColor" />
                </IconButton>
              </Tooltip>
            </Stack>
            <Stack sx={{ gap: 0.25 }}>
              {pendingHires > 0 && (
                <ButtonBase
                  onClick={() => openAgent(null)}
                  sx={{
                    justifyContent: 'flex-start',
                    gap: 1,
                    px: 1,
                    py: 0.625,
                    borderRadius: '8px',
                    bgcolor: alpha(theme.palette.primary.main, 0.1),
                    color: 'primary.main',
                    '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.16) },
                  }}
                >
                  <Typography variant="caption" sx={{ fontWeight: 600, fontSize: '0.75rem' }}>
                    {pendingHires} proposed hire{pendingHires === 1 ? '' : 's'} to review
                  </Typography>
                </ButtonBase>
              )}
              {activeTeam.map((a) => (
                <TeamRow key={a.id} agent={a} runs={runs[`${root}::${a.id}`]} onClick={() => openAgent(a.id)} />
              ))}
              {activeTeam.length === 0 && pendingHires === 0 && (
                <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, py: 0.5, lineHeight: 1.45 }}>
                  {mode === 'team'
                    ? 'No agents yet. Build your team with + — each gets its own prompt, model and tools, and they work on your tasks together.'
                    : master
                      ? `No agents yet. ${master.name} proposes them as the work needs, or add one with +.`
                      : 'No agents yet. Add one with +, or brief a master agent to build the team.'}
                </Typography>
              )}
            </Stack>
          </Box>
        )}

      </Box>
      </>
      )}

      {/* Pinned to the bottom, folded: the archive is history, so it takes
          one line until asked for, and scrolls within its own space when
          open instead of pushing the team off the sidebar. */}
      {myArchive.length > 0 && (
        <Box sx={{ flexShrink: 0, borderTop: 1, borderColor: 'divider', py: 0.5 }}>
          <ButtonBase
            onClick={() => setArchiveOpen((v) => !v)}
            aria-expanded={archiveOpen}
            sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', gap: 0.75, px: 2, py: 0.5 }}
          >
            <Typography variant="caption" color="text.secondary" sx={SECTION_LABEL_SX}>
              Archive
            </Typography>
            <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
              {myArchive.length}
            </Typography>
            <Box sx={{ flex: 1 }} />
            <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
              {archiveOpen ? 'Hide' : 'Show'}
            </Typography>
          </ButtonBase>
          <Collapse in={archiveOpen} unmountOnExit>
            <Box sx={{ maxHeight: '38vh', overflowY: 'auto' }}>
              <Typography variant="caption" color="text.disabled" sx={{ display: 'block', px: 2, pb: 0.75, fontSize: '0.6875rem', lineHeight: 1.4 }}>
                Threads from before this project had one conversation. Kept exactly as they were, read-only.
              </Typography>
              <GroupedByMonth
                items={myArchive}
                dateOf={(s) => s.created_at}
                render={(s: SessionInfo) => (
                  <DayRow
                    key={s.id}
                    active={viewing?.id === s.id}
                    label={s.name || 'Untitled'}
                    preview={new Date(s.created_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}
                    onClick={() => void view(s.id, s.name || 'Untitled conversation')}
                  />
                )}
              />
            </Box>
          </Collapse>
        </Box>
      )}
      </>
      )}

      {master && <CharterDialog master={master} open={charterOpen} onClose={() => setCharterOpen(false)} />}
      <OnboardingDialog
        open={briefOpen}
        project={activeProject}
        onClose={() => setBriefOpen(false)}
      />
    </Stack>
  )
}

/**
 * Threads — separate conversations. General's, and a threads project's:
 * work that is a series of separate questions rather than one line of work
 * held together by a master or a team.
 */
function GeneralThreads({ onExport }: { onExport: () => void }) {
  const theme = useTheme()
  const threads = useDaysStore((s) => s.threads)
  const activeId = useDaysStore((s) => s.todayId)
  const viewing = useDaysStore((s) => s.viewing)
  const newThread = useDaysStore((s) => s.newThread)
  const openThread = useDaysStore((s) => s.openThread)
  const renameThread = useDaysStore((s) => s.renameThread)
  const deleteThread = useDaysStore((s) => s.deleteThread)
  const busy = useAgentStore((s) => s.isBusy)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)

  const commitRename = (id: string) => {
    const name = draft.trim()
    setRenaming(null)
    if (name) void renameThread(id, name)
  }
  // Picking a thread leaves a General agent's chat, if one is open.
  const generalRoot = useGeneralAgents((s) => s.root)
  const agentOpen = useBotsStore((s) => (generalRoot ? (s.channel[generalRoot] ?? 'room') !== 'room' : false))
  const leaveAgent = () => {
    if (generalRoot && agentOpen) useBotsStore.getState().setChannel(generalRoot, 'room')
  }

  return (
    <>
      <Stack direction="row" sx={{ alignItems: 'center', px: 2, pb: 0.75, gap: 0.25, flexShrink: 0 }}>
        <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
          Threads
        </Typography>
        <Tooltip title="Copy this thread as markdown">
          <IconButton size="small" aria-label="Copy as markdown" onClick={onExport}>
            <ExportLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      <Box sx={{ px: 1.5, pb: 1, flexShrink: 0 }}>
        <ButtonBase
          onClick={() => {
            leaveAgent()
            void newThread()
          }}
          disabled={busy}
          sx={{
            display: 'flex',
            width: '100%',
            justifyContent: 'flex-start',
            gap: 1,
            px: 1.25,
            py: 0.75,
            borderRadius: '8px',
            border: 1,
            borderColor: 'divider',
            fontSize: '0.8125rem',
            fontWeight: 500,
            '&:hover': { bgcolor: 'action.hover' },
            '&:disabled, &[aria-disabled="true"]': { opacity: 0.5 },
          }}
        >
          <AddLinear size={14} color="currentColor" />
          New thread
        </ButtonBase>
      </Box>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', pb: 1 }}>
        {threads.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 2, py: 1 }}>
            No threads yet.
          </Typography>
        ) : (
          <GroupedByMonth
            items={threads}
            dateOf={(t) => t.created_at}
            render={(t: SessionInfo) => {
              const active = !viewing && !agentOpen && t.id === activeId
              if (renaming === t.id) {
                return (
                  <Box key={t.id} sx={{ px: 0.5 }}>
                    <InputBase
                      autoFocus
                      fullWidth
                      value={draft}
                      onChange={(e) => setDraft(e.target.value)}
                      onBlur={() => commitRename(t.id)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') commitRename(t.id)
                        if (e.key === 'Escape') setRenaming(null)
                      }}
                      sx={{ fontSize: '0.8125rem', border: 1, borderColor: 'primary.main', borderRadius: '8px', px: 1, py: 0.25 }}
                    />
                  </Box>
                )
              }
              return (
                <Stack
                  key={t.id}
                  direction="row"
                  sx={{ alignItems: 'center', position: 'relative', '&:hover .thread-actions': { opacity: 1 } }}
                >
                  <Tooltip title="Double-click to rename" enterDelay={1200} placement="right">
                    <ButtonBase
                      onClick={() => {
                        leaveAgent()
                        void openThread(t.id)
                      }}
                      onDoubleClick={() => {
                        setRenaming(t.id)
                        setDraft(t.name)
                      }}
                      disabled={busy && !active}
                      aria-current={active ? 'true' : undefined}
                      sx={{
                        flex: 1,
                        minWidth: 0,
                        justifyContent: 'flex-start',
                        px: 1,
                        py: 0.625,
                        borderRadius: '8px',
                        bgcolor: active ? alpha(theme.palette.primary.main, 0.1) : 'transparent',
                        '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
                      }}
                    >
                      <Typography
                        variant="body2"
                        noWrap
                        sx={{ fontSize: '0.8125rem', fontWeight: active ? 600 : 400, color: active ? 'primary.main' : 'text.primary', pr: 4 }}
                      >
                        {t.name || 'New chat'}
                      </Typography>
                    </ButtonBase>
                  </Tooltip>
                  <Stack
                    direction="row"
                    className="thread-actions"
                    sx={{
                      position: 'absolute',
                      right: 4,
                      opacity: confirmDelete === t.id ? 1 : 0,
                      transition: 'opacity 0.12s',
                      bgcolor: 'background.paper',
                      borderRadius: '6px',
                    }}
                  >
                    {confirmDelete === t.id ? (
                      <>
                        <ButtonBase
                          onClick={() => {
                            setConfirmDelete(null)
                            void deleteThread(t.id)
                          }}
                          sx={{ fontSize: '0.6875rem', color: 'error.main', px: 0.75, py: 0.25, borderRadius: '6px' }}
                        >
                          Delete
                        </ButtonBase>
                        <ButtonBase
                          onClick={() => setConfirmDelete(null)}
                          sx={{ fontSize: '0.6875rem', color: 'text.secondary', px: 0.75, py: 0.25, borderRadius: '6px' }}
                        >
                          Keep
                        </ButtonBase>
                      </>
                    ) : (
                      <Tooltip title="Delete thread">
                        <span>
                          <IconButton size="small" aria-label="Delete thread" onClick={() => setConfirmDelete(t.id)} disabled={busy && active}>
                            <TrashLinear size={13} color="currentColor" />
                          </IconButton>
                        </span>
                      </Tooltip>
                    )}
                  </Stack>
                </Stack>
              )
            }}
          />
        )}
      </Box>
    </>
  )
}

function GroupedByMonth<T>({
  items,
  dateOf: getDate,
  render,
}: {
  items: T[]
  dateOf: (item: T) => string
  render: (item: T) => React.ReactNode
}) {
  const groups: Array<{ month: string; items: T[] }> = []
  for (const item of items) {
    const month = monthOf(getDate(item))
    const last = groups[groups.length - 1]
    if (last && last.month === month) last.items.push(item)
    else groups.push({ month, items: [item] })
  }
  return (
    <>
      {groups.map((g) => (
        <Box key={g.month} sx={{ pb: 0.5 }}>
          <Typography
            variant="caption"
            color="text.disabled"
            sx={{ display: 'block', px: 2, pt: 0.75, pb: 0.25, fontSize: '0.6875rem', fontWeight: 600 }}
          >
            {g.month}
          </Typography>
          <Stack sx={{ px: 1, gap: 0.25 }}>{g.items.map(render)}</Stack>
        </Box>
      ))}
    </>
  )
}

/** One specialist in the sidebar: who it is and whether it is working now. */
function TeamRow({ agent, runs, onClick }: { agent: TeamAgent; runs: AgentRun[] | undefined; onClick: () => void }) {
  const theme = useTheme()
  const working = isWorking(runs)
  const current = runs?.find((r) => r.status === 'running')
  return (
    <ButtonBase
      onClick={onClick}
      sx={{
        display: 'flex',
        justifyContent: 'flex-start',
        alignItems: 'center',
        gap: 1,
        width: '100%',
        textAlign: 'left',
        px: 1,
        py: 0.625,
        borderRadius: '8px',
        '&:hover': { bgcolor: 'action.hover' },
        '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 1 },
      }}
    >
      <Box sx={{ position: 'relative', flexShrink: 0 }}>
        <Avatar name={agent.name} color={colorOf(agent)} size={22} />
        {working && (
          <Box
            sx={{
              position: 'absolute',
              right: -1,
              bottom: -1,
              width: 8,
              height: 8,
              borderRadius: '50%',
              bgcolor: 'primary.main',
              border: '2px solid',
              borderColor: 'background.paper',
            }}
          />
        )}
      </Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 500, fontSize: '0.8125rem' }} noWrap>
          {agent.name}
        </Typography>
        <Typography variant="caption" color={working ? 'primary.main' : 'text.secondary'} sx={{ fontSize: '0.6875rem' }} noWrap>
          {working ? current?.description || 'Working…' : agent.role || 'Idle'}
        </Typography>
      </Stack>
    </ButtonBase>
  )
}

function DayRow({
  label,
  preview,
  active,
  primary = false,
  onClick,
}: {
  label: string
  preview: string
  active: boolean
  primary?: boolean
  onClick: () => void
}) {
  const theme = useTheme()
  return (
    <ButtonBase
      onClick={onClick}
      aria-current={active ? 'true' : undefined}
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'flex-start',
        width: '100%',
        textAlign: 'left',
        gap: 0.125,
        px: primary ? 1.5 : 1,
        py: primary ? 1 : 0.625,
        borderRadius: primary ? '12px' : '8px',
        bgcolor: active ? alpha(theme.palette.primary.main, primary ? 0.14 : 0.1) : 'transparent',
        border: primary ? 1 : 0,
        borderColor: active ? alpha(theme.palette.primary.main, 0.4) : 'divider',
        transition: 'background-color 0.12s',
        '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
        '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 1 },
      }}
    >
      <Typography
        variant="body2"
        sx={{ fontWeight: primary || active ? 600 : 500, fontSize: primary ? '0.875rem' : '0.8125rem', color: active ? 'primary.main' : 'text.primary' }}
        noWrap
      >
        {label}
      </Typography>
      <Typography variant="caption" color="text.secondary" noWrap sx={{ width: '100%', fontSize: '0.75rem' }}>
        {preview}
      </Typography>
    </ButtonBase>
  )
}
