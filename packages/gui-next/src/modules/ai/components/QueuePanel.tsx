import { useEffect, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import { CloseCircleLinear, AddLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import { contentToText, type AgentMessage } from '../lib/types'

function messageText(content: AgentMessage['content']): string {
  return contentToText(content) || '(empty)'
}

/** Inline strip of what's actually queued in the backend (steering +
 * follow-up) — sits in normal document flow right above the composer input,
 * each row greyed-out with its own dismiss (X), no separate floating panel
 * or expand/collapse chrome. */
export function QueuePanel() {
  const isBusy = useAgentStore((s) => s.isBusy)
  const queued = useAgentStore((s) => s.queuedMessages)
  const refreshQueues = useAgentStore((s) => s.refreshQueues)
  const removeQueued = useAgentStore((s) => s.removeQueued)
  const queueFollowUp = useAgentStore((s) => s.queueFollowUp)
  const [followUpText, setFollowUpText] = useState('')

  const addFollowUp = () => {
    const text = followUpText.trim()
    if (!text) return
    void queueFollowUp(text)
    setFollowUpText('')
  }

  useEffect(() => {
    void refreshQueues()
  }, [refreshQueues])

  // Light polling while the agent is busy (the only time the queue's
  // contents can change from outside this panel's own actions).
  useEffect(() => {
    if (!isBusy) return
    const id = window.setInterval(() => void refreshQueues(), 2000)
    return () => window.clearInterval(id)
  }, [isBusy, refreshQueues])

  const rows: { key: 'steering' | 'followUp'; index: number; text: string }[] = [
    ...queued.steering.map((m, i) => ({ key: 'steering' as const, index: i, text: messageText(m.content) })),
    ...queued.followUp.map((m, i) => ({ key: 'followUp' as const, index: i, text: messageText(m.content) })),
  ]

  if (rows.length === 0 && !isBusy) return null

  return (
    <Stack sx={{ gap: 0.5, px: 0.5 }}>
      {rows.map((row) => (
        <Stack
          key={`${row.key}-${row.index}`}
          direction="row"
          sx={{
            alignItems: 'center',
            gap: 1,
            px: 1.25,
            py: 0.625,
            borderRadius: 2,
            bgcolor: 'rgba(255,255,255,0.03)',
            border: 1,
            borderColor: 'rgba(255,255,255,0.06)',
          }}
        >
          <Typography variant="caption" noWrap sx={{ flex: 1, minWidth: 0, color: 'text.secondary' }}>
            {row.text}
          </Typography>
          <Typography variant="caption" sx={{ color: 'text.disabled', fontSize: '0.625rem', flexShrink: 0 }}>
            {row.key === 'steering' ? 'next up' : 'after this turn'}
          </Typography>
          <IconButton size="small" onClick={() => void removeQueued(row.key, row.index)} sx={{ p: 0.25, flexShrink: 0 }}>
            <CloseCircleLinear size={13} color="currentColor" />
          </IconButton>
        </Stack>
      ))}
      {isBusy && (
        <Stack
          direction="row"
          sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.375, borderRadius: 2, border: 1, borderColor: 'rgba(255,255,255,0.06)' }}
        >
          <InputBase
            fullWidth
            placeholder="Queue a follow-up for after this turn…"
            value={followUpText}
            onChange={(e) => setFollowUpText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') addFollowUp()
            }}
            sx={{ fontSize: '0.75rem' }}
          />
          <IconButton size="small" onClick={addFollowUp} disabled={!followUpText.trim()} sx={{ flexShrink: 0 }}>
            <AddLinear size={13} color="currentColor" />
          </IconButton>
        </Stack>
      )}
    </Stack>
  )
}
