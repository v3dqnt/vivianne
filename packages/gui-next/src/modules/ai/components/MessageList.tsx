import { useEffect, useMemo, useRef, useState } from 'react'
import type { AgentMessage } from '../lib/types'
import { findAssignments, type Assignment } from '../lib/review'
import { ChangeSummary } from './ChangeSummary'
import { Virtualizer } from 'virtua'
import { Box, Typography, Dialog, DialogTitle, DialogContent, DialogContentText, DialogActions, Button } from '../../../ui'
import { useAgentStore } from '../store/agentStore'
import { MessageBubble } from './MessageBubble'
import { ThinkingIndicator } from './ThinkingIndicator'
import { TodoPanel } from './TodoPanel'

/**
 * The conversation transcript.
 *
 * With no props it renders the live conversation from the agent store. Given
 * `messages` + `readOnly`, it renders a saved conversation instead — a past
 * day, or an archived thread — through the same renderer, so history looks
 * exactly like it did live. Read-only drops everything that would act on the
 * live agent: revert, edit, the busy indicator and the todo panel.
 */
export function MessageList({
  messages: savedMessages,
  earlier,
  earlierStart = 0,
  onLoadOlder,
  loadingOlder = false,
  readOnly = false,
  sessionId,
  emptyText,
}: {
  messages?: AgentMessage[]
  /** Messages compacted out of the model's context, shown read-only above
   * the live ones so nothing the user said ever disappears. */
  earlier?: AgentMessage[]
  /** Archive index of `earlier[0]`. Above 0, older pages can be loaded. */
  earlierStart?: number
  onLoadOlder?: () => void
  loadingOlder?: boolean
  /** Replaces the default empty-state line. */
  emptyText?: string
  readOnly?: boolean
  /** The saved conversation's id, when showing one — its change reviews live
   * under that id rather than today's. */
  sessionId?: string
} = {}) {
  const liveMessages = useAgentStore((s) => s.messages)
  const messages = savedMessages ?? liveMessages
  // True for the one render in which older archive messages were prepended.
  const lastStart = useRef(earlierStart)
  const prepending = earlierStart < lastStart.current
  useEffect(() => {
    lastStart.current = earlierStart
  }, [earlierStart])
  const liveBusy = useAgentStore((s) => s.isBusy)
  // A saved conversation is never "busy", whatever today's live one is doing.
  const isBusy = readOnly ? false : liveBusy
  const liveStreamingIndex = useAgentStore((s) => s.streamingIndex)
  const streamingIndex = readOnly ? null : liveStreamingIndex
  const revertToIndex = useAgentStore((s) => s.revertToIndex)
  const setDraft = useAgentStore((s) => s.setDraft)
  const containerRef = useRef<HTMLDivElement>(null)
  // Whether the user was already at (or near) the bottom before this update —
  // streaming shouldn't yank them back down if they scrolled up to reread
  // something earlier in a long reply.
  const nearBottomRef = useRef(true)
  const scrollRafRef = useRef<number | null>(null)

  // Which message closes which assignment, so each one's change review can
  // sit under its last message. Recomputed only when the transcript changes.
  const assignmentsByEnd = useMemo(() => {
    const map = new Map<number, Assignment>()
    for (const a of findAssignments(messages)) if (a.editedFiles) map.set(a.endIndex, a)
    return map
  }, [messages])

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

  if (messages.length === 0 && !earlier?.length) {
    return (
      <Box sx={{ position: 'relative', flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {!readOnly && <TodoPanel />}
        <Typography variant="body2" color="text.secondary">
          {emptyText ?? (readOnly ? 'Nothing was said in this conversation.' : 'Ask anything to get started.')}
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
  // Archived messages carry no actions: reverting or editing is only
  // meaningful for what is still in the model's context.
  // Keyed by position in the whole archive, so loading an older page in
  // front doesn't remount (and re-highlight) everything already on screen.
  const earlierItems = (earlier ?? []).map((m, i) => (
    <Box key={`e${earlierStart + i}`} sx={{ pb: 1.5 }}>
      <MessageBubble message={m} index={i} />
    </Box>
  ))
  if (earlierStart > 0 && onLoadOlder) {
    earlierItems.unshift(
      <Box key="load-older" sx={{ display: 'flex', justifyContent: 'center', pb: 1.5 }}>
        <Button size="small" color="inherit" onClick={onLoadOlder} disabled={loadingOlder} sx={{ color: 'text.secondary' }}>
          {loadingOlder ? 'Loading…' : `Show earlier messages (${earlierStart})`}
        </Button>
      </Box>,
    )
  }
  const items = messages.map((m, i) => (
    // Index key is acceptable here: this list is append-only within a
    // session (no reordering/filtering), which is exactly the case index
    // keys are safe for.
    <Box key={i} sx={{ pb: 1.5 }}>
      {/* Omitting the callbacks is what hides a bubble's revert and edit
          actions — history is not editable. */}
      <MessageBubble
        message={m}
        index={i}
        streaming={isBusy && streamingIndex === i}
        onRevert={readOnly ? undefined : setRevertTarget}
        onEdit={readOnly ? undefined : startEdit}
      />
      {/* The review appears once an assignment is over — never under the one
          still running, whose change set isn't final yet. */}
      {assignmentsByEnd.has(i) && !(isBusy && assignmentsByEnd.get(i)!.toMs === null) && (
        <Box sx={{ mt: 1 }}>
          <ChangeSummary assignment={assignmentsByEnd.get(i)!} sessionId={sessionId} readOnly={readOnly} />
        </Box>
      )}
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
    // TodoPanel is a SIBLING of the scroller, not a child of it. An
    // absolutely-positioned element inside a scroll container is positioned
    // against that container's content box, so it scrolled away with the
    // messages — the todo list vanished exactly when a long run made it most
    // useful. Anchoring it to this non-scrolling wrapper keeps it pinned.
    <Box sx={{ position: 'relative', flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {!readOnly && <TodoPanel />}
      <Box ref={containerRef} sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 2, py: 1.5 }}>
        {/* Capped and centred so the reading measure stays stable as the window
            resizes, and lines up with the Composer's matching maxWidth below it. */}
        {/* The conversation is content, so it's selectable; the chrome
            around it isn't (see baselineExtras). */}
        <Box className="select-text" sx={{ maxWidth: 880, mx: 'auto' }}>
          {/* `shift` while an older page is being put in front, so the view
              stays on what the user was reading instead of jumping. */}
          <Virtualizer scrollRef={containerRef} shift={prepending}>
            {earlierItems.length ? [...earlierItems, ...items] : items}
          </Virtualizer>
        </Box>
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
