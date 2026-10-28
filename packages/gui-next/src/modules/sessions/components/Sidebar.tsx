import { useEffect, useMemo, useState } from 'react'
import { useStore } from 'zustand'
import { Box, Stack, Typography, IconButton, InputBase, ButtonBase, Tooltip, Collapse, Orb, alpha, useTheme } from '../../../ui'
import { ExportLinear, AddLinear, TrashLinear } from '../../../icons'
import { useProjectsStore, projectColor, modeOf, GENERAL_PROJECT_ID } from '../store/projectsStore'
import { SidebarMods } from '../../mods/components/SidebarMods'
import { useDaysStore, holderOf } from '../store/daysStore'
import { useAgentStore, agentStoreFor } from '../../ai/store/agentStore'
import { contentToText, type AgentMessage } from '../../ai/lib/types'
import { isAppText } from '../../ai/components/HandoffDivider'
import { surfaceCard } from '../../../theme'
import type { SessionInfo } from '../lib/types'
import { useMasterStore } from '../store/masterStore'
import { CharterDialog } from './CharterDialog'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { GitStatusChip } from '../../git/components/GitActions'
import { OnboardingDialog } from './OnboardingDialog'
import { useTeamStore } from '../../team/store/teamStore'
import { TreeTeam } from '../../team/components/TreeTeam'
import { useGeneralAgents } from '../../team/store/generalAgentsStore'
import { useBotsStore } from '../../team/store/botsStore'

export const SIDEBAR_WIDTH = 260

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
  const refreshArchive = useDaysStore((s) => s.refreshArchive)

  const [archiveOpen, setArchiveOpen] = useState(false)

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
  const focusAgent = useTeamStore((s) => s.focus)
  const openAgent = (id: string | null) => {
    if (!root) return
    focusAgent(root, id)
    // The Agents panel is gone; picking an agent opens its conversation in
    // the chat instead.
    if (id) useBotsStore.getState().setChannel(root, id)
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
        {/* The folder the agent works in. General works across the whole
            computer, so it shows none. */}
        {!isGeneral && (
        <Stack direction="row" sx={{ ml: -0.75, alignItems: 'center', gap: 0.25, flexWrap: 'wrap' }}>
          <WorkspaceSelector />
          <GitStatusChip />
        </Stack>
        )}
      </Stack>


      {threaded ? (
        <>
          <GeneralThreads onExport={exportCurrent} />
        </>
      ) : (
      <>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', pb: 1 }}>
        {root && activeProject && (
          <TreeTeam
            root={root}
            team={activeTeam}
            master={master ? { name: master.name, color: projectColor(activeProject) } : null}
            pendingHires={pendingHires}
            onHire={() => openAgent(null)}
            onCharter={() => {
              // The master may have edited the charter since it was loaded.
              void loadMaster(root)
              setCharterOpen(true)
            }}
          />
        )}
      </Box>
      </>
      )}

      {/* Sections mods add (Settings → Mods). */}
      <Box sx={{ flexShrink: 0, maxHeight: '45vh', overflowY: 'auto' }}>
        <SidebarMods />
      </Box>

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
                      {!active && <ThreadStatus sessionId={t.id} />}
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
/** A dot on a thread running in the background: working, or waiting on you. */
function ThreadStatus({ sessionId }: { sessionId: string }) {
  const holder = holderOf(sessionId)
  return holder === undefined ? null : <ThreadStatusDot threadId={holder} />
}

function ThreadStatusDot({ threadId }: { threadId: string }) {
  const store = agentStoreFor(threadId)
  const busy = useStore(store, (s) => s.isBusy)
  const waiting = useStore(store, (s) => !!(s.pendingPermission || s.pendingQuestion || s.pendingConfirmation))
  if (!busy && !waiting) return null
  const label = waiting ? 'Waiting on you' : 'Working'
  return (
    <Tooltip title={label}>
      <Box sx={{ display: 'flex', flexShrink: 0, ml: -3.5, mr: 3 }}>
        <Orb state={waiting ? 'listening' : 'working'} px={14} label={label} />
      </Box>
    </Tooltip>
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
        borderRadius: '8px',
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
