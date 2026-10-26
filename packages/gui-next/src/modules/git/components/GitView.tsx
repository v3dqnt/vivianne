import { useEffect } from 'react'
import { create } from 'zustand'
import { Box, Chip, IconButton, Link, Stack, Tooltip, Typography, useTheme } from '../../../ui'
import { GithubLinear, RefreshLinear } from '../../../icons'
import { surfaceCard } from '../../../theme'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { BranchIndicator } from '../../ai/components/BranchIndicator'
import { useGitStore } from '../store/gitStore'
import { ReviewPanel } from './ReviewPanel'

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

/**
 * The repository of the project's folder, in one place: its branch and
 * GitHub remote, what changed (with the diff review), and its history.
 */
export function GitView() {
  const theme = useTheme()
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const remote = useGitStore((s) => s.remote)
  const commits = useGitStore((s) => s.commits)
  const branches = useGitStore((s) => s.branches)
  const loading = useGitStore((s) => s.loading)
  const refresh = useGitStore((s) => s.refresh)

  useEffect(() => {
    if (root) void refresh(root)
  }, [root, refresh])

  const card = { ...surfaceCard(theme.palette.mode), minHeight: 0, overflow: 'hidden' } as const
  const local = branches.filter((b) => !b.isRemote).length

  return (
    <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0, gap: 0.75, mb: 1, mx: 1 }}>
      <Stack direction="row" sx={{ ...card, alignItems: 'center', gap: 1, px: 1.5, py: 0.75, flexShrink: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 650 }} noWrap>
          {root ? root.split(/[\\/]/).filter(Boolean).pop() : 'No folder'}
        </Typography>
        <BranchIndicator />
        <Typography variant="caption" color="text.secondary">
          {local} branch{local === 1 ? '' : 'es'}
        </Typography>
        <Box sx={{ flex: 1 }} />
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
            <IconButton size="small" aria-label="Refresh" disabled={!root || loading} onClick={() => root && void refresh(root)}>
              <RefreshLinear size={14} color="currentColor" />
            </IconButton>
          </span>
        </Tooltip>
      </Stack>

      <Stack direction="row" sx={{ flex: 1, minHeight: 0, gap: 0.75 }}>
        <Box sx={{ ...card, flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          <ReviewPanel />
        </Box>
        <Stack sx={{ ...card, width: 340, flexShrink: 0 }}>
          <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.6875rem', px: 1.5, pt: 1.25, pb: 0.5 }}>
            History
          </Typography>
          <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', pb: 1 }}>
            {commits.length === 0 && (
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1.5 }}>
                {loading ? 'Loading…' : root ? 'No commits, or not a git repository.' : 'Open a project folder first.'}
              </Typography>
            )}
            {commits.map((c, i) => (
              <Stack key={c.hash} direction="row" sx={{ gap: 1, px: 1.5, py: 0.625, alignItems: 'flex-start', '&:hover': { bgcolor: 'action.hover' } }}>
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
        </Stack>
      </Stack>
    </Stack>
  )
}
