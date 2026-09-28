import { memo, useState } from 'react'
import { Box, Stack, Typography } from '../../../ui'
import { TaskLinear, ArrowDownLinear, ArrowUpLinear, TickCircleLinear, AddCircleLinear, StopCircleLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import type { AgentMessage } from '../lib/types'

interface TodoItem {
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
}

/** Pulls the most recent todo_write / todo_write_rich tool call's items out
 * of the live message list — no separate backend read needed, the todo
 * state already flows through as an ordinary tool call the model made.
 *
 * Memoized by the toolResult message object it came from: this runs on every
 * store notification (i.e. per streaming token), and returning the SAME array
 * reference while the source message is unchanged lets zustand's Object.is
 * check skip the subscriber's re-render entirely. */
const EMPTY_TODOS: TodoItem[] = []
let todosCache: { src: AgentMessage; todos: TodoItem[] } | null = null
function latestTodos(messages: AgentMessage[]): TodoItem[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m.type !== 'toolResult') continue
    if (m.toolName !== 'todo_write' && m.toolName !== 'todo_write_rich') continue
    const raw = m.arguments?.todos
    if (!Array.isArray(raw)) continue
    if (todosCache?.src === m) return todosCache.todos
    const todos = raw.map((t): TodoItem => ({
      content: (t as { content?: string; subject?: string })?.content ?? (t as { subject?: string })?.subject ?? '',
      status: ((t as { status?: string })?.status ?? 'pending') as TodoItem['status'],
    }))
    todosCache = { src: m, todos }
    return todos
  }
  return EMPTY_TODOS
}

/** Small floating widget showing the agent's live todo list — an
 * absolutely-positioned overlay that floats above the activity feed,
 * collapsed to a pill by default. Not part of the dock system.
 *
 * Memoized: MessageList re-renders per streamed token, and this widget sits
 * inside it — with zero props, `memo` lets it bail out of all of those. */
export const TodoPanel = memo(function TodoPanel() {
  const [expanded, setExpanded] = useState(false)
  const todos = useAgentStore((s) => latestTodos(s.messages))
  const doneCount = todos.filter((t) => t.status === 'completed').length

  if (todos.length === 0) return null

  const statusIcon = (status: TodoItem['status']) => {
    if (status === 'completed') return <TickCircleLinear size={13} color="currentColor" />
    if (status === 'cancelled') return <StopCircleLinear size={13} color="currentColor" />
    return <AddCircleLinear size={13} color="currentColor" />
  }

  return (
    <Box sx={{ position: 'absolute', top: 10, right: 10, zIndex: 20, width: 256, select: 'none' }}>
      <Box sx={{ borderRadius: 2, border: 1, borderColor: 'divider', bgcolor: 'background.paper', boxShadow: 3, overflow: 'hidden' }}>
        <Stack
          component="button"
          direction="row"
          onClick={() => setExpanded((v) => !v)}
          sx={{
            width: '100%',
            alignItems: 'center',
            gap: 1,
            px: 1.5,
            py: 0.75,
            border: 'none',
            bgcolor: 'transparent',
            cursor: 'pointer',
            color: 'text.secondary',
            '&:hover': { color: 'text.primary' },
            fontSize: '0.6875rem',
          }}
        >
          <TaskLinear size={13} color="currentColor" />
          <Typography component="span" variant="caption" sx={{ fontWeight: 600, color: 'text.primary' }}>
            {doneCount}/{todos.length}
          </Typography>
          <Typography component="span" variant="caption">
            done
          </Typography>
          <Box sx={{ ml: 'auto', display: 'flex' }}>
            {expanded ? <ArrowUpLinear size={10} color="currentColor" /> : <ArrowDownLinear size={10} color="currentColor" />}
          </Box>
        </Stack>

        {expanded && (
          <Box sx={{ borderTop: 1, borderColor: 'divider', p: 1.25, maxHeight: 256, overflowY: 'auto' }}>
            <Stack sx={{ gap: 0.75 }}>
              {todos.map((t, i) => (
                <Stack key={i} direction="row" sx={{ alignItems: 'flex-start', gap: 1, fontSize: '0.75rem' }}>
                  <Box sx={{ mt: 0.25, display: 'flex', flexShrink: 0, color: t.status === 'completed' ? 'text.secondary' : t.status === 'in_progress' ? 'primary.main' : 'text.disabled' }}>
                    {statusIcon(t.status)}
                  </Box>
                  <Typography
                    variant="caption"
                    sx={{
                      color: t.status === 'completed' ? 'text.secondary' : 'text.primary',
                      textDecoration: t.status === 'completed' ? 'line-through' : 'none',
                      fontWeight: t.status === 'in_progress' ? 600 : 400,
                    }}
                  >
                    {t.content}
                  </Typography>
                </Stack>
              ))}
            </Stack>
          </Box>
        )}
      </Box>
    </Box>
  )
})
