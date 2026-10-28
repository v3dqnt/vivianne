import { useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { GitStatusChip, GitQuickActions } from '../../git/components/GitActions'
import { useAppView } from '../../git/components/GitView'
import { useMasterStore } from '../../sessions/store/masterStore'
import { CharterDialog } from '../../sessions/components/CharterDialog'
import { Specialists } from './TeamStrip'

/** Every action in the column is the same row: left-aligned on one edge,
 * one line, muted until hovered. */
const ROW = {
  display: 'flex',
  width: '100%',
  justifyContent: 'flex-start',
  alignItems: 'center',
  height: 28,
  px: '8px',
  borderRadius: '6px',
  fontSize: '13px',
  fontWeight: 400,
  color: 'text.secondary',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
} as const

const LABEL = { fontSize: '11px', fontWeight: 600, color: 'text.disabled', px: '8px', pb: '4px' } as const

/** A team project's own tools, on the chat's right: the folder it works in,
 * version control, and the charter. Mirrors the team on the left. */
export function ProjectTools({ root }: { root: string }) {
  const master = useMasterStore((s) => s.byRoot[root])
  const [charter, setCharter] = useState(false)
  return (
    <Box sx={{ position: 'absolute', right: 24, top: 24, width: 184, zIndex: 3 }}>
      <Stack sx={{ gap: '20px' }}>
        <Box>
          <Typography sx={LABEL}>Workspace</Typography>
          <Box sx={{ ml: '-2px' }}>
            <WorkspaceSelector />
          </Box>
        </Box>

        <Box>
          <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', gap: '8px', pb: '4px' }}>
            <Typography sx={{ ...LABEL, pb: 0 }}>Version control</Typography>
            <Box sx={{ minWidth: 0, '& .ui-ButtonBase': { height: 20, px: '4px' } }}>
              <GitStatusChip />
            </Box>
          </Stack>
          {/* The quick actions (commit, sync, pull request, review), drawn as
              the column's rows rather than as their own buttons. */}
          <Box
            sx={{
              '& > div': { flexDirection: 'column', alignItems: 'stretch', gap: 0 },
              '& > div > span:not(:has(button))': { display: 'none' },
              '& .ui-Button': {
                ...ROW,
                border: 0,
                bgcolor: 'transparent',
                boxShadow: 'none',
                transform: 'none',
                '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
              },
              '& .ui-Button:first-of-type': { color: 'text.primary', fontWeight: 500 },
            }}
          >
            <GitQuickActions />
          </Box>
          <ButtonBase onClick={() => useAppView.setState({ view: 'git' })} sx={ROW}>
            History and pull requests
          </ButtonBase>
        </Box>

        <Specialists root={root} rowSx={ROW} labelSx={LABEL} />

        {master && (
          <Box>
            <Typography sx={LABEL}>Project</Typography>
            <ButtonBase
              onClick={() => {
                void useMasterStore.getState().load(root)
                setCharter(true)
              }}
              sx={ROW}
            >
              Charter
            </ButtonBase>
          </Box>
        )}
      </Stack>
      {master && <CharterDialog master={master} open={charter} onClose={() => setCharter(false)} />}
    </Box>
  )
}
