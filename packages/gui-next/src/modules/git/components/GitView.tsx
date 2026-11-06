import { useEffect, useState } from 'react'
import { create } from 'zustand'
import { Box, ButtonBase, IconButton, Link, Stack, Tooltip, Typography, useTheme } from '../../../ui'
import { GithubLinear, RefreshLinear, ArrowLeftLinear } from '../../../icons'
import { surfaceCard } from '../../../theme'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { BranchIndicator } from '../../ai/components/BranchIndicator'
import { useGitStore } from '../store/gitStore'
import { ReviewPanel } from './ReviewPanel'
import { Actions, Issues, PullRequests, Releases } from './GitHubPanels'
import { GitQuickActions, PrChip } from './GitActions'
import { T, u } from './gitStyle'
import { useGitStatus, changedCount } from '../store/gitStatusStore'

/** What the main area shows: home, the chat and its panels, the agents, or
 * the repository. The title bar's home icon reaches home. */
export const useAppView = create<{ view: 'home' | 'chat' | 'agents' | 'git' }>(() => ({ view: 'home' }))

function when(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const days = Math.floor((Date.now() - d.getTime()) / 86_400_000)
  if (days < 1) return d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  if (days < 7) return `${days}d ago`
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

type Tab = 'changes' | 'prs' | 'actions' | 'issues' | 'releases' | 'history'
const TABS: Array<[Tab, string]> = [
  ['changes', 'Changes'],
  ['prs', 'Pull requests'],
  ['actions', 'Actions'],
  ['issues', 'Issues'],
  ['releases', 'Releases'],
  ['history', 'History'],
]

/**
 * The repository of the project's folder, in one place: its branch and
 * GitHub remote, what changed (with the diff review), pull requests with
 * their checks, Actions runs with their jobs and logs, issues, releases, and
 * the history. GitHub comes through the same token the agent uses.
 */
export function GitView() {
  const theme = useTheme()
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const remote = useGitStore((s) => s.remote)
  const commits = useGitStore((s) => s.commits)
  const branches = useGitStore((s) => s.branches)
  const loading = useGitStore((s) => s.loading)
  const refresh = useGitStore((s) => s.refresh)
  const checkout = useGitStore((s) => s.checkoutBranch)
  const [tab, setTab] = useState<Tab>('changes')
  const pr = useGitStatus((s) => s.pr)
  const openPrs = useGitStatus((s) => s.openPrs)
  const openIssues = useGitStatus((s) => s.openIssues)
  const failingRuns = useGitStatus((s) => s.failingRuns)
  const changes = useGitStatus((s) => (s.summary ? changedCount(s.summary) : 0))
  // What waits on each tab: changes to commit, open pull requests and
  // issues, workflows whose latest run on this branch failed.
  const count = (t: Tab) => (t === 'changes' ? changes : t === 'prs' ? (openPrs ?? 0) : t === 'issues' ? (openIssues ?? 0) : t === 'actions' ? (failingRuns ?? 0) : 0)
  // A refresh remounts the open tab, so it loads again.
  const [epoch, setEpoch] = useState(0)

  useEffect(() => {
    if (root) void refresh(root)
  }, [root, refresh])

  const card = { ...surfaceCard(theme.palette.mode), minHeight: 0, overflow: 'hidden' } as const
  const local = branches.filter((b) => !b.isRemote).length
  const folder = root ? root.split(/[\\/]/).filter(Boolean).pop() : 'No folder'

  return (
    <Stack sx={{ ...card, flex: 1, minWidth: 0, mb: 1, mx: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: u(2), px: u(3), height: 40, flexShrink: 0, minWidth: 0 }}>
        <Tooltip title="Back to chat">
          <IconButton
            size="small"
            sx={{ width: 24, height: 24 }}
            aria-label="Back to chat"
            onClick={() => useAppView.setState({ view: 'chat' })}
          >
            <ArrowLeftLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Typography sx={{ ...T.title }} noWrap>
          {folder}
        </Typography>
        <BranchIndicator />
        <Typography sx={{ ...T.meta, color: 'text.secondary', whiteSpace: 'nowrap' }}>
          {local} branch{local === 1 ? '' : 'es'}
        </Typography>
        {pr && <PrChip pr={pr} />}
        <Box sx={{ flex: 1 }} />
        <GitQuickActions />
        {remote?.githubUrl ? (
          <Link href={remote.githubUrl} target="_blank" rel="noreferrer" underline="hover" sx={{ display: 'flex', alignItems: 'center', gap: u(1), ...T.small, color: 'text.secondary', whiteSpace: 'nowrap' }}>
            <GithubLinear size={14} color="currentColor" />
            {remote.githubRepo}
          </Link>
        ) : (
          remote?.url && (
            <Typography noWrap sx={{ ...T.small, color: 'text.secondary', maxWidth: 320 }}>
              {remote.url}
            </Typography>
          )
        )}
        <Tooltip title="Refresh">
          <span>
            <IconButton
              size="small"
              sx={{ width: 24, height: 24 }}
              aria-label="Refresh"
              disabled={!root || loading}
              onClick={() => {
                if (root) void refresh(root)
                setEpoch((n) => n + 1)
              }}
            >
              <RefreshLinear size={14} color="currentColor" />
            </IconButton>
          </span>
        </Tooltip>
      </Stack>

      <Stack direction="row" role="tablist" sx={{ px: u(2), gap: u(1), height: 32, alignItems: 'stretch', borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        {TABS.map(([id, label]) => (
          <ButtonBase
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            sx={{
              px: u(2),
              gap: u(1),
              ...T.small,
              fontWeight: tab === id ? 600 : 500,
              color: tab === id ? 'text.primary' : 'text.secondary',
              borderBottom: 2,
              borderColor: tab === id ? 'primary.main' : 'transparent',
              mb: '-1px',
              '&:hover': { color: 'text.primary' },
            }}
          >
            {label}
            {count(id) > 0 && (
              <Box component="span" sx={{ minWidth: 16, height: 16, px: u(1), borderRadius: '4px', fontSize: '0.6875rem', lineHeight: '16px', fontWeight: 500, textAlign: 'center', boxSizing: 'border-box', bgcolor: id === 'actions' ? 'error.main' : 'action.selected', color: id === 'actions' ? 'error.contrastText' : 'text.secondary' }}>
                {count(id)}
              </Box>
            )}
          </ButtonBase>
        ))}
      </Stack>

      <Box key={`${tab}-${epoch}-${root}`} sx={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
        {tab === 'changes' && <ReviewPanel />}
        {tab === 'prs' && (
          <PullRequests
            root={root}
            onCheckout={async (branch, pr) => {
              if (root) await checkout(root, branch, pr)
            }}
          />
        )}
        {tab === 'actions' && <Actions root={root} />}
        {tab === 'issues' && <Issues root={root} />}
        {tab === 'releases' && <Releases root={root} />}
        {tab === 'history' && (
          <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', py: u(1) }}>
            {commits.length === 0 && (
              <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(4) }}>
                {loading ? 'Loading…' : root ? 'No commits, or not a git repository.' : 'Open a project folder first.'}
              </Typography>
            )}
            {commits.map((c, i) => (
              <Stack key={c.hash} direction="row" sx={{ gap: u(3), px: u(4), height: 28, alignItems: 'center', '&:hover': { bgcolor: 'action.hover' } }}>
                <Box sx={{ width: 8, height: 8, borderRadius: '50%', flexShrink: 0, bgcolor: i === 0 ? 'warning.main' : 'text.disabled' }} />
                <Box component="span" sx={{ ...T.meta, ...T.mono, color: 'text.secondary', flexShrink: 0, width: 56 }}>
                  {c.hash.slice(0, 7)}
                </Box>
                {i === 0 && (
                  <Box component="span" sx={{ ...T.meta, fontWeight: 600, px: u(1), borderRadius: '4px', border: 1, borderColor: 'warning.main', color: 'warning.main', flexShrink: 0 }}>
                    HEAD
                  </Box>
                )}
                <Typography noWrap sx={{ ...T.body, flex: 1, minWidth: 0 }} title={c.message}>
                  {c.message}
                </Typography>
                <Typography noWrap sx={{ ...T.meta, color: 'text.secondary', flexShrink: 0, maxWidth: 160 }}>
                  {c.author}
                </Typography>
                <Typography sx={{ ...T.meta, color: 'text.secondary', flexShrink: 0, width: 64, textAlign: 'right' }}>
                  {when(c.date)}
                </Typography>
              </Stack>
            ))}
          </Box>
        )}
      </Box>
    </Stack>
  )
}
