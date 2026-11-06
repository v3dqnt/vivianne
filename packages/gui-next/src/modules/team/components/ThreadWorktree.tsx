import { useCallback, useEffect, useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { call, on } from '../../../ipc'
import { activeThreadId } from '../../ai/store/activeThread'
import { useDaysStore } from '../../sessions/store/daysStore'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'

interface Worktree {
  workspace: string
  root: string
  branch: string
  base: string
}

const ROW = {
  display: 'flex',
  width: '100%',
  justifyContent: 'flex-start',
  alignItems: 'center',
  height: 28,
  px: '8px',
  borderRadius: '6px',
  fontSize: '0.8125rem',
  color: 'text.secondary',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
  '&:disabled': { opacity: 0.5 },
} as const

/** The open thread's own branch: where it works, and the two ways to finish with it. */
export function ThreadWorktree() {
  const openId = useDaysStore((s) => s.todayId)
  const [wt, setWt] = useState<Worktree | null>(null)
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ text: string; bad: boolean } | null>(null)

  const read = useCallback(async () => {
    setWt((await call<Worktree | null>('cmd_thread_worktree', { threadId: activeThreadId() }).catch(() => null)) ?? null)
  }, [])

  useEffect(() => {
    setSaid(null)
    void read()
  }, [openId, read])
  // The first turn of a thread makes its worktree; follow it into the workspace.
  useEffect(() => {
    const off = on<{ id: string; workspace: string }>('thread:worktree', ({ id, workspace }) => {
      if (id !== useDaysStore.getState().todayId) return
      void useWorkspaceStore.getState().setWorkspace([workspace], useWorkspaceStore.getState().accessMode)
      void read()
    })
    return () => void off.then((f) => f())
  }, [read])

  if (!wt) return null

  const finish = async (action: 'merge' | 'remove') => {
    setBusy(true)
    setSaid(null)
    try {
      const text = (await call<string>('cmd_thread_worktree_finish', { threadId: activeThreadId(), action })) ?? ''
      setSaid({ text, bad: false })
      // Back in the project's own folder once the branch is gone.
      if (action === 'merge' || /removed/i.test(text)) {
        const project = await call<string | null>('cmd_thread_project_path', { threadId: activeThreadId() }).catch(() => null)
        if (project) await useWorkspaceStore.getState().setWorkspace([project], useWorkspaceStore.getState().accessMode)
      }
    } catch (e) {
      setSaid({ text: String(e instanceof Error ? e.message : e), bad: true })
    } finally {
      setBusy(false)
      void read()
    }
  }

  return (
    <Box className="wt" sx={{ borderTop: 1, borderColor: 'divider', pt: '12px' }}>
      <Typography sx={{ fontSize: '0.6875rem', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'text.disabled', px: '8px', pb: '4px' }}>This thread's branch</Typography>
      <Typography noWrap title={wt.branch} sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem', px: '8px' }}>
        {wt.branch}
      </Typography>
      <Typography noWrap sx={{ fontSize: '0.75rem', color: 'text.secondary', px: '8px', pb: '4px' }}>
        cut from {wt.base || 'the project'}
      </Typography>
      <Stack>
        <ButtonBase disabled={busy} onClick={() => void finish('merge')} sx={ROW}>
          Merge into {wt.base || 'the project'}
        </ButtonBase>
        <ButtonBase disabled={busy} onClick={() => void finish('remove')} sx={ROW}>
          Remove the branch
        </ButtonBase>
      </Stack>
      {said && (
        <Typography sx={{ fontSize: '0.75rem', lineHeight: 1.5, px: '8px', pt: '4px', color: said.bad ? 'warning.main' : 'text.secondary' }}>{said.text}</Typography>
      )}
    </Box>
  )
}
