import Dialog from '@mui/material/Dialog'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Box from '@mui/material/Box'
import Chip from '@mui/material/Chip'
import Tooltip from '@mui/material/Tooltip'
import { Hierarchy2Linear, RefreshLinear, CloseCircleLinear } from '../../../icons'
import { useGitStore } from '../store/gitStore'

/** Commit history table. ponytail: the "Graph" column is a colored dot per
 * row, not a rendered branch-line graph (multi-branch topology lines) — add
 * that if a real multi-branch view turns out to matter; `git log` already
 * gives every commit's parent hashes to build one from, this just doesn't
 * render it yet. */
export function GitGraphModal({ open, onClose, root }: { open: boolean; onClose: () => void; root: string | null }) {
  const commits = useGitStore((s) => s.commits)
  const branches = useGitStore((s) => s.branches)
  const refresh = useGitStore((s) => s.refresh)
  const loading = useGitStore((s) => s.loading)

  const branchesByHead = branches.filter((b) => b.isCurrent).map((b) => b.name)

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth slotProps={{ paper: { sx: { height: 560, borderRadius: '18px' } } }}>
      <Stack sx={{ height: '100%', minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 2, py: 1.5, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
          <Hierarchy2Linear size={16} color="currentColor" />
          <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
            Git Graph
          </Typography>
          <Tooltip title="Refresh">
            <IconButton size="small" onClick={() => root && void refresh(root)} disabled={loading}>
              <RefreshLinear size={14} color="currentColor" />
            </IconButton>
          </Tooltip>
          <IconButton size="small" onClick={onClose}>
            <CloseCircleLinear size={16} color="currentColor" />
          </IconButton>
        </Stack>

        <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
          <Box sx={{ display: 'grid', gridTemplateColumns: '48px 1fr 140px 140px 96px', alignItems: 'center', px: 2, py: 1, borderBottom: 1, borderColor: 'divider', position: 'sticky', top: 0, bgcolor: 'background.paper' }}>
            {['Graph', 'Description', 'Date', 'Author', 'Commit'].map((h) => (
              <Typography key={h} variant="caption" color="text.secondary" sx={{ fontWeight: 600 }}>
                {h}
              </Typography>
            ))}
          </Box>
          {commits.length === 0 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>
              {loading ? 'Loading…' : 'No commits.'}
            </Typography>
          ) : (
            commits.map((c, i) => (
              <Box
                key={c.hash}
                sx={{ display: 'grid', gridTemplateColumns: '48px 1fr 140px 140px 96px', alignItems: 'center', px: 2, py: 1, '&:hover': { bgcolor: 'action.hover' } }}
              >
                <Box sx={{ width: 10, height: 10, borderRadius: '50%', bgcolor: i === 0 ? 'warning.main' : 'text.disabled' }} />
                <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0 }}>
                  {i === 0 && <Chip size="small" label="HEAD" variant="outlined" sx={{ height: 18, fontSize: '0.625rem', borderColor: 'warning.main', color: 'warning.main' }} />}
                  {i === 0 && branchesByHead.map((b) => <Chip key={b} size="small" label={b} sx={{ height: 18, fontSize: '0.625rem' }} />)}
                  <Typography variant="body2" noWrap sx={{ fontSize: '0.8125rem' }}>
                    {c.message}
                  </Typography>
                </Stack>
                <Typography variant="caption" color="text.secondary">
                  {c.date}
                </Typography>
                <Typography variant="caption" color="text.secondary" noWrap>
                  {c.author}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace' }}>
                  {c.hash.slice(0, 7)}
                </Typography>
              </Box>
            ))
          )}
        </Box>
      </Stack>
    </Dialog>
  )
}
