import { useEffect, useState } from 'react'
import { Box, ButtonBase, IconButton, Stack } from '../../../ui'
import { TrashLinear } from '../../../icons'
import { useDaysStore } from '../store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { useThreadGit } from '../../git/store/threadGitStore'
import { changedCount } from '../../git/store/gitStatusStore'
import type { SessionInfo } from '../lib/types'


/** One rhythm for every row: the same height, inset and corner. */
const ROW = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: '10px',
  width: '100%',
  minHeight: 32,
  px: '10px',
  borderRadius: '8px',
  fontSize: '0.875rem',
  textAlign: 'left',
  color: 'text.primary',
  '&:hover': { bgcolor: 'action.hover' },
  // Keyboard focus is as visible as hover, never lost on the dark card.
  '&:focus-visible': { outline: '2px solid', outlineColor: 'primary.main', outlineOffset: '-2px' },
} as const


/** A thread's name as a person reads it: the app's own markers and
 * markdown stripped from a title that was cut from a message. */
export function titleOf(thread: SessionInfo): string {
  const clean = (thread.name ?? '').replace(/<[^>]*>/g, ' ').replace(/[*_`#]+/g, '').replace(/\s+/g, ' ').trim()
  return clean || 'New chat'
}

/** A thread: its name, and on the right what waits in its worktree. */
export function ThreadRow({ thread, active, onOpen }: { thread: SessionInfo; active: boolean; onOpen: () => void }) {
  const [confirming, setConfirming] = useState(false)
  const busy = useAgentStore((s) => s.isBusy)
  const folder = thread.workspace_folders?.[0]
  const git = useThreadGit((s) => (folder ? s.byPath[folder] : null))
  const ensure = useThreadGit((s) => s.ensure)
  useEffect(() => {
    if (folder) ensure([folder])
  }, [folder, ensure])
  // Threads that share a folder share its changes: the count is shown on the
  // open thread, where it is about the work on screen.
  const changed = git && active ? changedCount(git) : 0
  const title = titleOf(thread)
  return (
    <Stack direction="row" sx={{ position: 'relative', '&:hover .row-delete': { opacity: 1 }, '&:hover .row-meta': { opacity: 0 } }}>
      <ButtonBase
        onClick={onOpen}
        aria-current={active ? 'true' : undefined}
        title={git ? `${title}\n${git.branch}` : title}
        sx={{
          ...ROW,
          pr: '36px',
          bgcolor: active ? 'action.selected' : 'transparent',
          fontWeight: active ? 500 : 400,
          color: active ? 'text.primary' : 'text.secondary',
          '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover', color: 'text.primary' },
        }}
      >
        <Box component="span" sx={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {title}
        </Box>
      </ButtonBase>
      <Box className="row-meta" sx={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)', fontSize: '0.75rem', color: git?.conflicted ? 'error.main' : 'warning.main', pointerEvents: 'none', transition: 'opacity 120ms' }}>
        {changed > 0 ? `+${changed}` : ''}
      </Box>
      <Box className="row-delete" sx={{ position: 'absolute', right: 4, top: 4, opacity: confirming ? 1 : 0, transition: 'opacity 120ms' }}>
        {confirming ? (
          <Stack direction="row" sx={{ bgcolor: 'background.paper', borderRadius: '6px' }}>
            <ButtonBase
              onClick={() => {
                setConfirming(false)
                void useDaysStore.getState().deleteThread(thread.id)
              }}
              sx={{ fontSize: '0.75rem', color: 'error.main', px: '8px', height: 28, borderRadius: '6px' }}
            >
              Delete
            </ButtonBase>
            <ButtonBase onClick={() => setConfirming(false)} sx={{ fontSize: '0.75rem', color: 'text.secondary', px: '8px', height: 28, borderRadius: '6px' }}>
              Keep
            </ButtonBase>
          </Stack>
        ) : (
          <IconButton size="small" aria-label="Delete thread" disabled={busy && active} onClick={() => setConfirming(true)}>
            <TrashLinear size={14} color="currentColor" />
          </IconButton>
        )}
      </Box>
    </Stack>
  )
}
