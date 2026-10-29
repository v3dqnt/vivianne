import { useMemo, useState } from 'react'
import { Box, ButtonBase, Popover, Stack, Tooltip, Typography } from '../../../ui'
import { DangerLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import { contentToText, type AgentMessage } from '../lib/types'
import { TOOLBAR_PILL } from '../lib/toolbarPill'

export interface RunError {
  text: string
  ts: number
}

/** A turn that failed (rate limit, bad key, provider down) ends in an error
 * message. They are kept out of the transcript and listed here instead. */
export function runErrors(messages: AgentMessage[]): RunError[] {
  const out: RunError[] = []
  for (const m of messages) {
    if (m.type === 'assistant' && m.stopReason === 'error') {
      const text = (m.errorMessage || contentToText(m.content)).replace(/^Error:\s*/, '').trim()
      if (text) out.push({ text, ts: m.timestamp })
    }
  }
  return out.reverse()
}

const when = (ts: number) => {
  const d = new Date(ts)
  const today = d.toDateString() === new Date().toDateString()
  return today
    ? d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    : d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

/**
 * The conversation's errors behind one icon by the composer: it shows only
 * when there are any, with a count of the ones you haven't opened yet, and
 * opens onto the list, newest first.
 */
export function ErrorsButton({ sx }: { sx?: Record<string, unknown> }) {
  const messages = useAgentStore((s) => s.messages)
  const errors = useMemo(() => runErrors(messages), [messages])
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [seenUntil, setSeenUntil] = useState(0)
  if (errors.length === 0) return null
  const unseen = errors.filter((e) => e.ts > seenUntil).length

  return (
    <>
      <Tooltip title={unseen ? `${unseen} new error${unseen === 1 ? '' : 's'}` : 'Errors in this conversation'}>
        <ButtonBase
          aria-label="Errors"
          onClick={(e) => {
            setAnchor(e.currentTarget)
            setSeenUntil(errors[0].ts)
          }}
          sx={{ ...TOOLBAR_PILL, color: unseen ? 'error.main' : 'text.secondary', ...sx }}
        >
          <DangerLinear size={14} color="currentColor" />
          {unseen > 0 && <Box component="span">{unseen}</Box>}
        </ButtonBase>
      </Tooltip>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'right' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'right' }}
        slotProps={{ paper: { sx: { mt: -1, width: 340, maxHeight: 360, borderRadius: '12px', border: 1, borderColor: 'divider', overflow: 'auto' } } }}
      >
        <Stack sx={{ p: '12px', gap: '8px' }}>
          <Typography sx={{ fontSize: '13px', fontWeight: 600 }}>Errors</Typography>
          {errors.map((e, i) => (
            <Stack key={i} sx={{ gap: '2px', px: '10px', py: '8px', borderRadius: '8px', bgcolor: 'action.hover' }}>
              <Typography sx={{ fontSize: '12px', color: 'error.main', lineHeight: 1.45, wordBreak: 'break-word' }}>{e.text}</Typography>
              <Typography sx={{ fontSize: '11px', color: 'text.disabled' }}>{when(e.ts)}</Typography>
            </Stack>
          ))}
        </Stack>
      </Popover>
    </>
  )
}
