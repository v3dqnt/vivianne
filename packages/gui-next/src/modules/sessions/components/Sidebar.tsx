import { useEffect, useMemo, useState } from 'react'
import { useStore } from 'zustand'
import { Box, Stack, Typography, IconButton, InputBase, ButtonBase, Tooltip, Collapse, Orb, alpha, useTheme } from '../../../ui'
import { ExportLinear, AddLinear, TrashLinear, ArrowDownLinear, Hierarchy2Linear } from '../../../icons'
import { useProjectsStore, GENERAL_PROJECT_ID } from '../store/projectsStore'
import { SidebarMods } from '../../mods/components/SidebarMods'
import { useDaysStore, holderOf } from '../store/daysStore'
import { useAgentStore, agentStoreFor } from '../../ai/store/agentStore'
import { contentToText, type AgentMessage } from '../../ai/lib/types'
import { isAppText } from '../../ai/components/HandoffDivider'
import { surfaceCard } from '../../../theme'
import type { SessionInfo } from '../lib/types'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { GitStatusChip } from '../../git/components/GitActions'
import { useGitStatus, changedCount } from '../../git/store/gitStatusStore'
import { useThreadGit } from '../../git/store/threadGitStore'
import { MONO_FONT } from '../../git/components/gitStyle'
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
  fontWeight: 500,
  letterSpacing: 0,
  textTransform: 'none',
  fontSize: '0.75rem',
} as const

/**
 * The active project: its threads, and the archive of threads from before.
 *
 * There is no "New session" and no list of days. A threads project's
 * conversations are separate chats; the archive is history, read-only.
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

  const [detailsOpen, setDetailsOpen] = useState(false)

  // The archive belongs to projects the same way threads did: by the
  // assignment map. General gets the ones never assigned to anything.
  const myArchive = useMemo(() => {
    const mine = archive.filter((s) =>
      isGeneral ? !sessionProjectMap[s.id] : sessionProjectMap[s.id] === activeProjectId,
    )
    return mine
  }, [archive, sessionProjectMap, activeProjectId, isGeneral])


  if (!activeProject && !isGeneral) {
    return (
      <Stack component="nav" sx={{ ...SIDEBAR_CARD_SX, p: 2, gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
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
        {/* The project's name; its folder and git fold under it.
            General works across the whole computer, so it has none. */}
        <ButtonBase
          onClick={() => !isGeneral && setDetailsOpen((v) => !v)}
          disabled={isGeneral}
          aria-expanded={isGeneral ? undefined : detailsOpen}
          sx={{ alignSelf: 'flex-start', maxWidth: '100%', gap: 0.5, ml: -0.5, px: 0.5, py: 0.25, borderRadius: '6px', '&:hover': { bgcolor: isGeneral ? 'transparent' : 'action.hover' } }}
        >
          <Typography variant="subtitle2" sx={{ fontWeight: 500, minWidth: 0, letterSpacing: '-0.01em' }} noWrap title={activeProject?.name ?? 'General'}>
            {activeProject?.name ?? 'General'}
          </Typography>
          {!isGeneral && (
            <Box sx={{ display: 'flex', color: 'text.secondary', transform: detailsOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
              <ArrowDownLinear size={12} color="currentColor" />
            </Box>
          )}
        </ButtonBase>
        {!isGeneral && (
          <Collapse in={detailsOpen}>
            <Stack sx={{ gap: 0.5, pt: 0.5 }}>
              <Stack direction="row" sx={{ ml: -0.75, alignItems: 'center', gap: 0.25, flexWrap: 'wrap' }}>
                <WorkspaceSelector />
                <GitStatusChip />
              </Stack>
            </Stack>
          </Collapse>
        )}
      </Stack>


      <GeneralThreads />

      {/* Sections mods add (Settings → Mods). */}
      <Box sx={{ flexShrink: 0, maxHeight: '45vh', overflowY: 'auto' }}>
        <SidebarMods />
      </Box>

      {/* Pinned to the bottom, folded: the archive is history, so it takes
          one line until asked for, and scrolls within its own space when
          open instead of pushing the threads off the sidebar. */}
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
    </Stack>
  )
}

/**
 * Threads — separate conversations. General's, and a project's: work that is
 * a series of separate questions rather than one line of work.
 */
/** `labelSx`: the heading's look where the list sits beside the chat rather than in the sidebar. */
export function GeneralThreads({ labelSx }: { labelSx?: object } = {}) {
  const projectName = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId)?.name)
  const threads = useDaysStore((s) => s.threads)
  const activeId = useDaysStore((s) => s.todayId)
  const viewing = useDaysStore((s) => s.viewing)
  const newThread = useDaysStore((s) => s.newThread)
  const openThread = useDaysStore((s) => s.openThread)
  const renameThread = useDaysStore((s) => s.renameThread)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [draft, setDraft] = useState('')

  const exportCurrent = () => {
    const title = viewing ? viewing.label : `${projectName ?? 'General'} — conversation`
    const messages = viewing ? viewing.messages : [...useDaysStore.getState().earlier, ...useAgentStore.getState().messages]
    void navigator.clipboard.writeText(toMarkdown(title, messages)).catch(() => {
      // WebView2 can refuse clipboard access; nothing useful to show.
    })
  }

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
  // Ask git about each thread's folder once the list is known; the rows then
  // read their answer from the store.
  const ensureGit = useThreadGit((s) => s.ensure)
  const threadFolders = useMemo(() => threads.map((t) => t.workspace_folders?.[0]), [threads])
  useEffect(() => {
    ensureGit(threadFolders)
  }, [ensureGit, threadFolders])

  return (
    <>
      <Stack direction="row" sx={{ alignItems: 'center', px: 1, pb: 0.75, gap: 0.25, flexShrink: 0 }}>
        <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1, ...labelSx }}>
          Threads
        </Typography>
        <Tooltip title="Copy this thread as markdown">
          <IconButton size="small" aria-label="Copy as markdown" onClick={exportCurrent}>
            <ExportLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      <Box sx={{ px: 0, pb: 1, flexShrink: 0 }}>
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
            px: 1,
            py: 0.75,
            borderRadius: '8px',
            bgcolor: 'transparent',
            border: 0,
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
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, py: 1 }}>
            No threads yet.
          </Typography>
        ) : (
          <GroupedByMonth
            inset={0}
            items={threads}
            dateOf={(t) => t.created_at}
            render={(t: SessionInfo) => {
              const active = !viewing && !agentOpen && t.id === activeId
              if (renaming === t.id) {
                return (
                  <Box key={t.id} sx={{ px: 1 }}>
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
                <ThreadRow
                  key={t.id}
                  thread={t}
                  active={active}
                  onOpen={() => {
                    leaveAgent()
                    void openThread(t.id)
                  }}
                  onRename={() => {
                    setRenaming(t.id)
                    setDraft(t.name)
                  }}
                />
              )
            }}
          />
        )}
      </Box>
    </>
  )
}

/**
 * One thread in the rail: the branch its work sits on and when it was asked,
 * the question itself, and how far that work has got — uncommitted, unpushed,
 * merged.
 */
function ThreadRow({
  thread,
  active,
  onOpen,
  onRename,
}: {
  thread: SessionInfo
  active: boolean
  onOpen: () => void
  onRename: () => void
}) {
  const [confirming, setConfirming] = useState(false)
  const deleteThread = useDaysStore((s) => s.deleteThread)
  const busy = useAgentStore((s) => s.isBusy)
  const folder = thread.workspace_folders?.[0]
  const git = useThreadGit((s) => (folder ? s.byPath[folder] : null))
  // Only the open project's repository has its pull request watched, and a
  // pull request is the one thing that says a branch's work has landed.
  const trackedRoot = useGitStatus((s) => s.root)
  const trackedPr = useGitStatus((s) => s.pr)
  const pr = folder && folder === trackedRoot ? trackedPr : null
  const changed = git ? changedCount(git) : 0
  const merged = pr?.state === 'merged'
  return (
    <Stack direction="row" sx={{ position: 'relative', '&:hover .thread-actions': { opacity: 1 } }}>
      <Tooltip title="Double-click to rename" enterDelay={1200} placement="right">
        <ButtonBase
          onClick={onOpen}
          onDoubleClick={onRename}
          aria-current={active ? 'true' : undefined}
          sx={{
            position: 'relative',
            flex: 1,
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'stretch',
            gap: '2px',
            pl: 1.5,
            // Room for the delete button, which rides the row's right edge
            // on hover.
            pr: 3.5,
            py: 0.875,
            borderRadius: '8px',
            bgcolor: active ? 'action.selected' : 'transparent',
            '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover' },
            // The one place the accent shows in the rail: which thread is open.
            '&::before': {
              content: '""',
              position: 'absolute',
              left: 3,
              top: 9,
              bottom: 9,
              width: 3,
              borderRadius: 1.6,
              bgcolor: 'primary.main',
              opacity: active ? 1 : 0,
            },
          }}
        >
          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0 }}>
            <Typography
              noWrap
              title={thread.name || 'New chat'}
              sx={{
                flex: 1,
                minWidth: 0,
                fontSize: '0.8125rem',
                fontWeight: active ? 600 : 500,
                color: 'text.primary',
                textAlign: 'left',
                lineHeight: 1.4,
              }}
            >
              {thread.name || 'New chat'}
            </Typography>
            {!active && <ThreadStatus sessionId={thread.id} />}
            <Typography sx={{ flexShrink: 0, fontSize: '0.6875rem', color: 'text.disabled' }}>
              {shortAgo(thread.created_at)}
            </Typography>
          </Stack>
          {git && (
            <Stack
              direction="row"
              sx={{ alignItems: 'center', gap: 0.75, minWidth: 0, fontSize: '0.6875rem', color: 'text.disabled' }}
            >
              <Box
                component="span"
                title={git.upstream ? `${git.branch} → ${git.upstream}` : git.branch}
                sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.5, minWidth: 0, color: 'text.secondary' }}
              >
                <Hierarchy2Linear size={11} color="currentColor" />
                <Box component="span" sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontFamily: MONO_FONT }}>
                  {git.branch || 'detached'}
                </Box>
              </Box>
              {changed > 0 ? (
                <Box
                  component="span"
                  title={`${changed} uncommitted change${changed === 1 ? '' : 's'}`}
                  sx={{ color: git.conflicted ? 'error.main' : 'warning.main' }}
                >
                  {changed} changed
                </Box>
              ) : (
                <Box component="span" title="Nothing uncommitted">
                  clean
                </Box>
              )}
              {git.ahead > 0 && (
                <Box component="span" title={`${git.ahead} to push`}>
                  ↑{git.ahead}
                </Box>
              )}
              {git.behind > 0 && (
                <Box component="span" title={`${git.behind} to pull`}>
                  ↓{git.behind}
                </Box>
              )}
              {pr && (
                <Box
                  component="span"
                  title={`#${pr.number} ${pr.title} · ${merged ? 'merged' : pr.state === 'closed' ? 'closed' : 'open'}`}
                  sx={{
                    color: merged
                      ? '#a371f7'
                      : pr.state === 'closed'
                        ? 'text.disabled'
                        : pr.checks === 'failing'
                          ? 'error.main'
                          : 'success.main',
                  }}
                >
                  {merged ? 'merged' : pr.state === 'closed' ? 'closed' : `#${pr.number}`}
                </Box>
              )}
            </Stack>
          )}
        </ButtonBase>
      </Tooltip>
      <Stack
        direction="row"
        className="thread-actions"
        sx={{
          position: 'absolute',
          right: 2,
          top: '50%',
          transform: 'translateY(-50%)',
          opacity: confirming ? 1 : 0,
          transition: 'opacity 0.12s',
          bgcolor: confirming ? 'background.paper' : 'transparent',
          borderRadius: '6px',
        }}
      >
        {confirming ? (
          <>
            <ButtonBase
              onClick={() => {
                setConfirming(false)
                void deleteThread(thread.id)
              }}
              sx={{ fontSize: '0.6875rem', color: 'error.main', px: 0.75, py: 0.25, borderRadius: '6px' }}
            >
              Delete
            </ButtonBase>
            <ButtonBase
              onClick={() => setConfirming(false)}
              sx={{ fontSize: '0.6875rem', color: 'text.secondary', px: 0.75, py: 0.25, borderRadius: '6px' }}
            >
              Keep
            </ButtonBase>
          </>
        ) : (
          <Tooltip title="Delete thread">
            <span>
              <IconButton size="small" aria-label="Delete thread" onClick={() => setConfirming(true)} disabled={busy && active}>
                <TrashLinear size={13} color="currentColor" />
              </IconButton>
            </span>
          </Tooltip>
        )}
      </Stack>
    </Stack>
  )
}

/** A thread's time, compact — `7m`, `3h`, `2d` — the way a list reads it. */
function shortAgo(iso: string): string {
  const s = (Date.now() - new Date(iso).getTime()) / 1000
  if (s < 60) return 'now'
  if (s < 3600) return `${Math.floor(s / 60)}m`
  if (s < 86400) return `${Math.floor(s / 3600)}h`
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d`
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function GroupedByMonth<T>({
  items,
  dateOf: getDate,
  render,
  inset = 1,
}: {
  items: T[]
  dateOf: (item: T) => string
  render: (item: T) => React.ReactNode
  /** Side inset. 0 when the list sits directly on the chrome, so the rows'
   * pills run the full width of the rail. */
  inset?: number
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
            color="text.secondary"
            sx={{ display: 'block', px: inset + 1, pt: 0.75, pb: 0.25, fontSize: '0.6875rem', fontWeight: 500 }}
          >
            {g.month}
          </Typography>
          <Stack sx={{ px: inset, gap: 0.25 }}>{g.items.map(render)}</Stack>
        </Box>
      ))}
    </>
  )
}

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
      <Box sx={{ display: 'flex', flexShrink: 0 }}>
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
