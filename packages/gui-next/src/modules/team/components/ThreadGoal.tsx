import { useCallback, useEffect, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography } from '../../../ui'
import { call, on } from '../../../ipc'
import { activeThreadId } from '../../ai/store/activeThread'
import { useDaysStore } from '../../sessions/store/daysStore'

interface Goal {
  objective: string
  phase: 'active' | 'paused' | 'blocked' | 'complete'
  blockedReason: string | null
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
} as const

/** What a person can do from each phase: the same legal moves the model has. */
const MOVES: Record<Goal['phase'], { label: string; action: string }[]> = {
  active: [{ label: 'Pause', action: 'pause' }, { label: 'Mark complete', action: 'complete' }],
  paused: [{ label: 'Resume', action: 'resume' }, { label: 'Mark complete', action: 'complete' }],
  blocked: [{ label: 'Resume', action: 'resume' }, { label: 'Mark complete', action: 'complete' }],
  complete: [],
}

/** The open thread's goal, and the controls for it: set, pause, resume, complete or clear, without a model turn. */
export function ThreadGoal() {
  const openId = useDaysStore((s) => s.todayId)
  const [goal, setGoal] = useState<Goal | null>(null)
  const [draft, setDraft] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const read = useCallback(() => {
    void call<Goal | null>('cmd_goal_get', { threadId: activeThreadId() }).then((g) => setGoal(g ?? null)).catch(() => setGoal(null))
  }, [])
  useEffect(() => {
    setDraft(null)
    setError(null)
    read()
    const off = on('goal:changed', read)
    return () => void off.then((f) => f())
  }, [openId, read])

  const run = async (action: string, text?: string) => {
    setError(null)
    try {
      setGoal((await call<Goal | null>('cmd_goal_do', { threadId: activeThreadId(), action, text })) ?? null)
      setDraft(null)
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e))
    }
  }

  if (draft !== null)
    return (
      <Stack sx={{ gap: '4px', px: '8px' }}>
        <InputBase
          autoFocus
          multiline
          value={draft}
          placeholder="What should it work toward?"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && draft.trim()) {
              e.preventDefault()
              void run(goal && !goal.phase.startsWith('complete') ? 'edit' : 'create', draft)
            }
            if (e.key === 'Escape') setDraft(null)
          }}
          sx={{ fontSize: '0.8125rem', border: 1, borderColor: 'divider', borderRadius: '8px', px: '8px', py: '4px' }}
        />
        {error && <Typography sx={{ fontSize: '0.75rem', color: 'warning.main' }}>{error}</Typography>}
      </Stack>
    )

  if (!goal)
    return (
      <>
        <ButtonBase onClick={() => setDraft('')} sx={ROW}>Set a goal</ButtonBase>
        {error && <Typography sx={{ fontSize: '0.75rem', color: 'warning.main', px: '8px' }}>{error}</Typography>}
      </>
    )

  return (
    <Stack>
      <Box sx={{ px: '8px', pb: '4px' }}>
        <Typography sx={{ fontSize: '0.6875rem', color: goal.phase === 'blocked' ? 'warning.main' : 'text.disabled' }}>
          {goal.phase === 'blocked' && goal.blockedReason ? `blocked: ${goal.blockedReason}` : goal.phase}
        </Typography>
        <Typography
          title={goal.objective}
          sx={{ fontSize: '0.8125rem', display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden', wordBreak: 'break-word' }}
        >
          {goal.objective}
        </Typography>
      </Box>
      {MOVES[goal.phase].map((m) => (
        <ButtonBase key={m.action} onClick={() => void run(m.action)} sx={ROW}>{m.label}</ButtonBase>
      ))}
      {goal.phase !== 'complete' && <ButtonBase onClick={() => setDraft(goal.objective)} sx={ROW}>Edit</ButtonBase>}
      <ButtonBase onClick={() => void run('clear')} sx={ROW}>{goal.phase === 'complete' ? 'Clear' : 'Drop the goal'}</ButtonBase>
      {error && <Typography sx={{ fontSize: '0.75rem', color: 'warning.main', px: '8px' }}>{error}</Typography>}
    </Stack>
  )
}
