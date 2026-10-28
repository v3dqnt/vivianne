import { useState } from 'react'
import { Box, Button, Stack, Typography } from '../../../ui'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { Avatar, faceOf } from './bits'

const NONE: TeamAgent[] = []

/** The lead's proposed hires, in the chat where you talk to it: approve one
 * and it joins the team; decline and it is dropped. */
export function HireRequests({ root }: { root: string }) {
  const proposed = useTeamStore((s) => s.byRoot[root] ?? NONE).filter((a) => a.status === 'proposed')
  const approve = useTeamStore((s) => s.approve)
  const remove = useTeamStore((s) => s.remove)
  const [busy, setBusy] = useState<string | null>(null)
  if (proposed.length === 0) return null

  const act = async (id: string, fn: () => Promise<void>) => {
    setBusy(id)
    try {
      await fn()
    } finally {
      setBusy(null)
    }
  }

  return (
    <Stack sx={{ gap: 1, width: '100%', maxWidth: 880, mx: 'auto', px: 2, pt: 1.5, flexShrink: 0 }}>
      {proposed.map((a) => (
        <Stack key={a.id} direction="row" sx={{ alignItems: 'flex-start', gap: 1.5, p: 1.5, borderRadius: '12px', border: 1, borderColor: 'divider', bgcolor: 'action.hover' }}>
          <Avatar {...faceOf(a)} size={32} />
          <Stack sx={{ flex: 1, minWidth: 0, gap: 0.25 }}>
            <Typography sx={{ fontSize: '13px', fontWeight: 600 }}>
              The lead wants to hire {a.name}
            </Typography>
            {a.role && (
              <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>{a.role}</Typography>
            )}
            {a.reason && (
              <Box component="span" sx={{ fontSize: '12px', color: 'text.secondary' }}>
                Why: {a.reason}
              </Box>
            )}
          </Stack>
          <Stack direction="row" sx={{ gap: 0.5, flexShrink: 0 }}>
            <Button size="small" color="inherit" disabled={busy === a.id} onClick={() => void act(a.id, () => remove(root, a.id))}>
              Decline
            </Button>
            <Button size="small" variant="contained" disableElevation disabled={busy === a.id} onClick={() => void act(a.id, () => approve(root, a.id))}>
              Hire
            </Button>
          </Stack>
        </Stack>
      ))}
    </Stack>
  )
}
