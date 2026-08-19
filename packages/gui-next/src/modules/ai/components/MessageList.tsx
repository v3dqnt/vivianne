import { useEffect, useRef, useState } from 'react'
import Stack from '@mui/material/Stack'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogContentText from '@mui/material/DialogContentText'
import DialogActions from '@mui/material/DialogActions'
import Button from '@mui/material/Button'
import { useAgentStore } from '../store/agentStore'
import { MessageBubble } from './MessageBubble'
import { ThinkingIndicator } from './ThinkingIndicator'
import { TodoPanel } from './TodoPanel'

export function MessageList() {
  const messages = useAgentStore((s) => s.messages)
  const isBusy = useAgentStore((s) => s.isBusy)
  const streamingIndex = useAgentStore((s) => s.streamingIndex)
  const revertToIndex = useAgentStore((s) => s.revertToIndex)
  const containerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  // Whether the user was already at (or near) the bottom before this update —
  // streaming shouldn't yank them back down if they scrolled up to reread
  // something earlier in a long reply.
  const nearBottomRef = useRef(true)
  const scrollRafRef = useRef<number | null>(null)

  const [revertTarget, setRevertTarget] = useState<number | null>(null)
  const [reverting, setReverting] = useState(false)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const NEAR_BOTTOM_PX = 80
    const onScroll = () => {
      nearBottomRef.current = el.scrollTop + el.clientHeight >= el.scrollHeight - NEAR_BOTTOM_PX
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => {
    if (!nearBottomRef.current) return
    // rAF-coalesced: `messages`/`isBusy` can change many times a second while
    // streaming (often 10-60/sec), and scrollIntoView forces a synchronous
    // layout — without coalescing, every token pays for its own layout pass.
    if (scrollRafRef.current !== null) cancelAnimationFrame(scrollRafRef.current)
    scrollRafRef.current = requestAnimationFrame(() => {
      bottomRef.current?.scrollIntoView({ block: 'end' })
      scrollRafRef.current = null
    })
    return () => {
      if (scrollRafRef.current !== null) cancelAnimationFrame(scrollRafRef.current)
    }
  }, [messages, isBusy])

  const confirmRevert = async () => {
    if (revertTarget === null) return
    setReverting(true)
    try {
      await revertToIndex(revertTarget)
    } finally {
      setReverting(false)
      setRevertTarget(null)
    }
  }

  if (messages.length === 0) {
    return (
      <Box sx={{ position: 'relative', flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <TodoPanel />
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
    <Box ref={containerRef} sx={{ position: 'relative', flex: 1, minHeight: 0, overflowY: 'auto', px: 1.5, py: 1.5 }}>
      <TodoPanel />
      <Stack spacing={1.5}>
        {messages.map((m, i) => (
          // Index key is acceptable here: this list is append-only within a
          // session (no reordering/filtering), which is exactly the case
          // index keys are safe for.
          <MessageBubble key={i} message={m} index={i} onRevert={setRevertTarget} />
        ))}
        {showTrailingIndicator && <ThinkingIndicator />}
        <div ref={bottomRef} />
      </Stack>

      <Dialog open={revertTarget !== null} onClose={() => setRevertTarget(null)} maxWidth="xs" fullWidth>
        <DialogTitle>Revert to this point?</DialogTitle>
        <DialogContent>
          <DialogContentText variant="body2">
            {revertTarget !== null && messages.length > 0
              ? `This removes ${messages.length - revertTarget} message${messages.length - revertTarget === 1 ? '' : 's'} after this one, including any tool calls and their results. This can't be undone.`
              : ''}
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setRevertTarget(null)} disabled={reverting} sx={{ textTransform: 'none' }}>
            Cancel
          </Button>
          <Button onClick={() => void confirmRevert()} disabled={reverting} color="error" sx={{ textTransform: 'none' }}>
            {reverting ? 'Reverting…' : 'Revert'}
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}
