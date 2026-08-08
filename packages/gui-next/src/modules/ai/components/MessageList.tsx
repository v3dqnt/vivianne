import { useEffect, useRef } from 'react'
import Stack from '@mui/material/Stack'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { useAgentStore } from '../store/agentStore'
import { MessageBubble } from './MessageBubble'
import { ThinkingIndicator } from './ThinkingIndicator'

export function MessageList() {
  const messages = useAgentStore((s) => s.messages)
  const isBusy = useAgentStore((s) => s.isBusy)
  const streamingIndex = useAgentStore((s) => s.streamingIndex)
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [messages, isBusy])

  if (messages.length === 0) {
    return (
      <Box sx={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <Typography variant="body2" color="text.secondary">
          Ask anything to get started.
        </Typography>
      </Box>
    )
  }

  // A gap exists between "user hit send" and the first `message_start`
  // event landing (and again between a tool call finishing and the next
  // reply starting) where no assistant bubble exists yet to show its own
  // in-bubble loading state — this row covers exactly that gap.
  const showTrailingIndicator = isBusy && streamingIndex === null

  return (
    <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 2, py: 2 }}>
      <Stack spacing={1.5}>
        {messages.map((m, i) => (
          // Index key is acceptable here: this list is append-only within a
          // session (no reordering/filtering), which is exactly the case
          // index keys are safe for.
          <MessageBubble key={i} message={m} />
        ))}
        {showTrailingIndicator && <ThinkingIndicator />}
        <div ref={bottomRef} />
      </Stack>
    </Box>
  )
}
