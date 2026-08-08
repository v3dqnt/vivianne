import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { Hierarchy2Linear } from 'mx-icons'
import { useWorkspaceStore } from '../store/workspaceStore'

/** Plain, non-interactive — there's no "switch branch" backend command, so this is a label, not a button. */
export function BranchIndicator() {
  const branch = useWorkspaceStore((s) => s.branch)
  if (!branch) return null

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1, color: 'text.secondary' }}>
      <Hierarchy2Linear size={13} color="currentColor" />
      <Typography variant="caption" noWrap sx={{ fontWeight: 500 }}>
        {branch}
      </Typography>
    </Stack>
  )
}
