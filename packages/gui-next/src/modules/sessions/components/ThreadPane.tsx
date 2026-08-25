import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import TextField from '@mui/material/TextField'
import { CloseCircleLinear, SendLinear } from '../../../icons'
import { call } from '../../../ipc'
import { getThreadStore } from '../../ai/store/threadRegistry'
import { useThreadsStore } from '../store/threadsStore'
import { useSessionsStore } from '../store/sessionsStore'
import { MessageBubble } from '../../ai/components/MessageBubble'
import type { AgentMessage } from '../../ai/lib/types'

/**
 * A secondary chat pane — genuinely independent of the primary sidebar
 * thread (its own `agentStore` instance, its own backend `AgentState` via
 * `AgentRegistry`), but deliberately minimal: no attachments, model
 * selector, or steering controls yet. Those live on the single-thread
 * `Composer`, which is wired to the primary store only — wiring the full
 * composer to be thread-aware is follow-up work, not bundled here.
 */
// Split panes have no revert affordance — one stable module-level noop rather
// than a fresh closure per render, which would defeat MessageBubble's memo
// and make every bubble re-render on every streamed token.
const NO_REVERT = () => {}

export function ThreadPane({ threadId, focused }: { threadId: string; focused: boolean }) {
  const store = getThreadStore(threadId)
  const messages = store((s) => s.messages)
  const isBusy = store((s) => s.isBusy)
  const init = store((s) => s.init)
  const sendMessage = store((s) => s.sendMessage)
  const close = useThreadsStore((s) => s.close)
  const focus = useThreadsStore((s) => s.focus)
  const setThreadWorkspace = useThreadsStore((s) => s.setThreadWorkspace)
  const session = useSessionsStore((s) => s.sessions.find((x) => x.id === threadId))

  // Bind this thread to the workspace its session was saved with, so the
  // dock panels follow the focused thread rather than one global workspace.
  useEffect(() => {
    if (session?.workspace_folders) setThreadWorkspace(threadId, session.workspace_folders)
  }, [threadId, session?.workspace_folders, setThreadWorkspace])

  const [text, setText] = useState('')
  const [loaded, setLoaded] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)
  const nearBottomRef = useRef(true)

  useEffect(() => {
    void init()
    // Hydrate this pane's own store from the session's saved transcript —
    // mirrors what `sessionsStore.loadSession` does for the primary thread,
    // scoped to this thread id so it lands in this pane's own state, not the
    // primary's.
    void call<AgentMessage[]>('cmd_agent_messages', { threadId }).then((msgs) => {
      if (msgs && msgs.length > 0) {
        store.setState({ messages: msgs })
      } else {
        void call('cmd_load_session', { id: threadId, threadId }).then(() =>
          call<AgentMessage[]>('cmd_agent_messages', { threadId }).then((loaded) => {
            if (loaded) store.setState({ messages: loaded })
          }),
        )
      }
      setLoaded(true)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadId])

  useEffect(() => {
    if (!nearBottomRef.current) return
    requestAnimationFrame(() => bottomRef.current?.scrollIntoView({ block: 'end' }))
  }, [messages, isBusy])

  const submit = () => {
    const trimmed = text.trim()
    if (!trimmed) return
    setText('')
    void sendMessage(trimmed)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submit()
    }
  }

  return (
    <Stack
      onClick={() => focus(threadId)}
      sx={{
        flex: 1,
        minWidth: 260,
        height: '100%',
        minHeight: 0,
        border: 1,
        borderColor: focused ? 'primary.main' : 'divider',
        borderRadius: 2,
        overflow: 'hidden',
        bgcolor: 'background.paper',
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.25, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Typography variant="caption" noWrap sx={{ flex: 1, minWidth: 0, fontWeight: 600 }}>
          {session?.name || 'Untitled'}
        </Typography>
        {isBusy && <CircularProgress size={12} thickness={5} />}
        <IconButton
          size="small"
          onClick={(e) => {
            e.stopPropagation()
            close(threadId)
          }}
        >
          <CloseCircleLinear size={14} color="currentColor" />
        </IconButton>
      </Stack>

      <Box
        onScroll={(e) => {
          const el = e.currentTarget
          nearBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80
        }}
        sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.5, py: 1.5 }}
      >
        {!loaded ? (
          <Typography variant="caption" color="text.secondary">
            Loading…
          </Typography>
        ) : (
          <Stack spacing={1}>
            {messages.map((m, i) => (
              <MessageBubble key={`${i}-${m.timestamp}`} message={m} index={i} onRevert={NO_REVERT} />
            ))}
            <div ref={bottomRef} />
          </Stack>
        )}
      </Box>

      <Box sx={{ p: 1, borderTop: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'flex-end', gap: 0.5 }}>
          <TextField
            fullWidth
            multiline
            maxRows={4}
            size="small"
            placeholder="Message this thread…"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
            variant="standard"
            slotProps={{ input: { disableUnderline: true } }}
            sx={{ px: 1, '& .MuiInputBase-root': { fontSize: '0.8125rem' } }}
          />
          <IconButton size="small" onClick={submit} disabled={!text.trim() || isBusy}>
            <SendLinear size={15} color="currentColor" />
          </IconButton>
        </Stack>
      </Box>
    </Stack>
  )
}
