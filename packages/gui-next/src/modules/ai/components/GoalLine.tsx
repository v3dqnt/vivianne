import { useCallback, useEffect, useState } from 'react'
import { Box, Stack, Typography } from '../../../ui'
import { call, on } from '../../../ipc'
import { activeThreadId } from '../store/activeThread'
import { useDaysStore } from '../../sessions/store/daysStore'

interface Goal {
  objective: string
  phase: 'active' | 'paused' | 'blocked' | 'complete'
  blockedReason: string | null
}

const PHASE: Record<Goal['phase'], string> = { active: 'working on it', paused: 'paused', blocked: 'blocked', complete: 'done' }

/**
 * The thread's goal, one quiet line above the composer while there is one. It is set and changed with `/goal` (`/goal <objective>`,
 * `/goal pause`, `resume`, `done`, `clear`), so the line itself carries no controls.
 */
export function GoalLine() {
  const openId = useDaysStore((s) => s.todayId)
  const [goal, setGoal] = useState<Goal | null>(null)
  const read = useCallback(() => {
    void call<Goal | null>('cmd_goal_get', { threadId: activeThreadId() }).then((g) => setGoal(g ?? null)).catch(() => setGoal(null))
  }, [])
  useEffect(() => {
    read()
    const off = on('goal:changed', read)
    return () => void off.then((f) => f())
  }, [read, openId])
  if (!goal || goal.phase === 'complete') return null
  return (
    <Stack direction="row" title="Change it with /goal" sx={{ alignItems: 'baseline', gap: '8px', px: '12px', minWidth: 0 }}>
      <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', flexShrink: 0 }}>Goal</Typography>
      <Typography noWrap sx={{ fontSize: '0.8125rem', color: 'text.primary', minWidth: 0, flex: 1 }}>
        {goal.objective}
      </Typography>
      <Box component="span" sx={{ fontSize: '0.75rem', color: goal.phase === 'blocked' ? 'warning.main' : 'text.secondary', flexShrink: 0 }}>
        {goal.phase === 'blocked' && goal.blockedReason ? `blocked: ${goal.blockedReason}` : PHASE[goal.phase]}
      </Box>
    </Stack>
  )
}
