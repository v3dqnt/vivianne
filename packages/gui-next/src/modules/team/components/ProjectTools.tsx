import { useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { DocumentTextLinear, GithubLinear } from '../../../icons'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { GitStatusChip, GitQuickActions } from '../../git/components/GitActions'
import { useAppView } from '../../git/components/GitView'
import { useMasterStore } from '../../sessions/store/masterStore'
import { CharterDialog } from '../../sessions/components/CharterDialog'

const LABEL_SX = { fontSize: '11px', fontWeight: 600, color: 'text.secondary', px: '8px' } as const
const LINK_SX = {
  justifyContent: 'flex-start',
  gap: '8px',
  height: 28,
  px: '8px',
  borderRadius: '6px',
  fontSize: '12px',
  color: 'text.secondary',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
} as const

/** A team project's own tools, on the chat's right: the folder it works in,
 * its version control, and the charter. Mirrors the team on the left. */
export function ProjectTools({ root }: { root: string }) {
  const master = useMasterStore((s) => s.byRoot[root])
  const [charter, setCharter] = useState(false)
  return (
    <Box sx={{ position: 'absolute', right: 12, top: 12, width: 180, zIndex: 3 }}>
      <Stack sx={{ gap: '16px' }}>
        <Stack sx={{ gap: '4px' }}>
          <Typography sx={LABEL_SX}>Workspace</Typography>
          <Box sx={{ px: '2px' }}>
            <WorkspaceSelector />
          </Box>
        </Stack>
        <Stack sx={{ gap: '4px' }}>
          <Typography sx={LABEL_SX}>Version control</Typography>
          <Box sx={{ px: '2px' }}>
            <GitStatusChip />
          </Box>
          <Box sx={{ px: '8px', '& > div': { flexDirection: 'column', alignItems: 'stretch' } }}>
            <GitQuickActions />
          </Box>
          <ButtonBase onClick={() => useAppView.setState({ view: 'git' })} sx={LINK_SX}>
            <GithubLinear size={13} color="currentColor" />
            Changes, PRs and history
          </ButtonBase>
        </Stack>
        {master && (
          <ButtonBase
            onClick={() => {
              void useMasterStore.getState().load(root)
              setCharter(true)
            }}
            sx={LINK_SX}
          >
            <DocumentTextLinear size={13} color="currentColor" />
            Charter
          </ButtonBase>
        )}
      </Stack>
      {master && <CharterDialog master={master} open={charter} onClose={() => setCharter(false)} />}
    </Box>
  )
}
