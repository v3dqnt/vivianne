import { useEffect, useState } from 'react'
import { create } from 'zustand'
import { Box, ButtonBase, Chip, IconButton, Link, Stack, Tooltip, Typography, useTheme } from '../../../ui'
import { GithubLinear, RefreshLinear } from '../../../icons'
import { surfaceCard } from '../../../theme'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { BranchIndicator } from '../../ai/components/BranchIndicator'
import { useGitStore } from '../store/gitStore'
import { ReviewPanel } from './ReviewPanel'
import { Actions, Issues, PullRequests, Releases } from './GitHubPanels'
import { GitQuickActions, PrChip } from './GitActions'
import { useGitStatus, changedCount } from '../store/gitStatusStore'

/** What the main area shows, picked in the title bar: the chat and its
 * panels, or the repository. */
export const useAppView = create<{ view: 'chat' | 'git' }>(() => ({ view: 'chat' }))

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
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, pt: 0.75, flexShrink: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 650 }} noWrap>
          {folder}
        </Typography>
        <BranchIndicator />
        <Typography variant="caption" color="text.secondary">
          {local} branch{local === 1 ? '' : 'es'}
        </Typography>
        {pr && <PrChip pr={pr} />}
        <Box sx={{ flex: 1 }} />
        <GitQuickActions />
        {remote?.githubUrl ? (
          <Link href={remote.githubUrl} target="_blank" rel="noreferrer" underline="hover" sx={{ display: 'flex', alignItems: 'center', gap: 0.5, fontSize: '0.75rem', color: 'text.secondary' }}>
            <GithubLinear size={14} color="currentColor" />
            {remote.githubRepo}
          </Link>
        ) : (
          remote?.url && (
            <Typography variant="caption" color="text.secondary" noWrap sx={{ maxWidth: 320 }}>
              {remote.url}
            </Typography>
          )
        )}
        <Tooltip title="Refresh">
          <span>
            <IconButton
              size="small"
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

      <Stack direction="row" role="tablist" sx={{ px: 1, gap: 0.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        {TABS.map(([id, label]) => (
          <ButtonBase
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            sx={{
              px: 1.25,
              py: 0.875,
              fontSize: '0.78125rem',
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
              <Box component="span" sx={{ ml: 0.625, px: 0.5, borderRadius: '8px', fontSize: '0.65625rem', fontWeight: 700, bgcolor: id === 'actions' ? 'error.main' : 'action.selected', color: id === 'actions' ? 'error.contrastText' : 'text.secondary' }}>
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
          <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', py: 0.5 }}>
            {commits.length === 0 && (
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>
                {loading ? 'Loading…' : root ? 'No commits, or not a git repository.' : 'Open a project folder first.'}
              </Typography>
            )}
            {commits.map((c, i) => (
              <Stack key={c.hash} direction="row" sx={{ gap: 1, px: 2, py: 0.75, alignItems: 'flex-start', '&:hover': { bgcolor: 'action.hover' } }}>
                <Box sx={{ mt: '6px', width: 7, height: 7, borderRadius: '50%', flexShrink: 0, bgcolor: i === 0 ? 'warning.main' : 'text.disabled' }} />
                <Stack sx={{ minWidth: 0, flex: 1 }}>
                  <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, minWidth: 0 }}>
                    {i === 0 && <Chip size="small" label="HEAD" variant="outlined" sx={{ height: 16, fontSize: '0.625rem', borderColor: 'warning.main', color: 'warning.main' }} />}
                    <Typography variant="body2" noWrap sx={{ fontSize: '0.8125rem' }} title={c.message}>
                      {c.message}
                    </Typography>
                  </Stack>
                  <Typography variant="caption" color="text.secondary" noWrap sx={{ fontSize: '0.6875rem' }}>
                    {c.author} · {when(c.date)} · <Box component="span" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>{c.hash.slice(0, 7)}</Box>
                  </Typography>
                </Stack>
              </Stack>
            ))}
          </Box>
        )}
      </Box>
    </Stack>
  )
}
