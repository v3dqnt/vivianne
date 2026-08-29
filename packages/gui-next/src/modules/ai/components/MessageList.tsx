import { useEffect, useRef, useState } from 'react'
import { Virtualizer } from 'virtua'
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
  const setDraft = useAgentStore((s) => s.setDraft)
  const containerRef = useRef<HTMLDivElement>(null)
  // Whether the user was already at (or near) the bottom before this update —
  // streaming shouldn't yank them back down if they scrolled up to reread
  // something earlier in a long reply.
  const nearBottomRef = useRef(true)
  const scrollRafRef = useRef<number | null>(null)

  const [revertTarget, setRevertTarget] = useState<number | null>(null)
  const [reverting, setReverting] = useState(false)

  /** Edit = revert the conversation to just before this message, then hand
   * its original text back to the composer to change and re-send. Reverting
   * is what makes the edit meaningful: leaving the old message (and every
   * reply built on it) in context would mean "editing" only ever appended a
   * near-duplicate. */
  const startEdit = async (index: number, text: string) => {
    await revertToIndex(index)
    setDraft(text)
  }

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
    // streaming (often 10-60/sec), and scrolling forces a synchronous layout —
    // without coalescing, every token pays for its own layout pass. Sets
    // `scrollTop` directly rather than `scrollIntoView` on a bottom sentinel:
    // `Virtualizer` below keeps the container's real `scrollHeight` correct
    // for the full (including unmounted) content, so this works the same
    // whether or not the last message is currently mounted.
    if (scrollRafRef.current !== null) cancelAnimationFrame(scrollRafRef.current)
    scrollRafRef.current = requestAnimationFrame(() => {
      const el = containerRef.current
      if (el) el.scrollTop = el.scrollHeight
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

  // A long thread otherwise mounts every message forever — DOM/memory grows
  // unbounded, and (compounding it) each mounted `ToolCallCard` collapse body
  // is measured by MUI's `Collapse` regardless of visibility. `Virtualizer`
  // renders only what's near the viewport, using `containerRef` (this
  // component's own scroll container) as the scrollable ancestor rather than
  // owning its own — so the existing near-bottom / scroll-position logic
  // above needs no changes to keep working.
  const items = messages.map((m, i) => (
    // Index key is acceptable here: this list is append-only within a
    // session (no reordering/filtering), which is exactly the case index
    // keys are safe for.
    <Box key={i} sx={{ pb: 1.5 }}>
      <MessageBubble message={m} index={i} onRevert={setRevertTarget} onEdit={startEdit} />
    </Box>
  ))
  if (showTrailingIndicator) {
    items.push(
      <Box key="trailing-indicator" sx={{ pb: 1.5 }}>
        <ThinkingIndicator />
      </Box>,
    )
  }

  return (
    <Box ref={containerRef} sx={{ position: 'relative', flex: 1, minHeight: 0, overflowY: 'auto', px: 2, py: 1.5 }}>
      <TodoPanel />
      {/* Capped and centred so the reading measure stays stable as the window
          resizes, and lines up with the Composer's matching maxWidth below it. */}
      <Box sx={{ maxWidth: 820, mx: 'auto' }}>
        <Virtualizer scrollRef={containerRef}>{items}</Virtualizer>
      </Box>

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
