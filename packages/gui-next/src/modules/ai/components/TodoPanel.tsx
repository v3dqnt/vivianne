import { memo, useState } from 'react'
import { Box, Stack, Typography } from '../../../ui'
import { TaskLinear, ArrowDownLinear, ArrowUpLinear, TickCircleLinear, StopCircleLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import type { AgentMessage } from '../lib/types'
import { todosOf, type TodoItem } from '../lib/checkpoints'

interface Plan {
  todos: TodoItem[]
  /** When each item was first marked done, by title. */
  doneAt: Map<string, number>
}

/** The plan from the most recent todo_write, with when each item got done —
 * read straight out of the transcript, which already holds every version of
 * the plan the agent wrote.
 *
 * Cached by the latest todo_write message: this runs on every store
 * notification (per streamed token), and returning the SAME object while
 * that message is unchanged lets zustand skip the re-render entirely. */
const EMPTY: Plan = { todos: [], doneAt: new Map() }
let cache: { src: AgentMessage; plan: Plan } | null = null
function latestPlan(messages: AgentMessage[]): Plan {
  let latest = -1
  for (let i = messages.length - 1; i >= 0; i--) {
    if (todosOf(messages[i])) {
      latest = i
      break
    }
  }
  if (latest < 0) return EMPTY
  if (cache?.src === messages[latest]) return cache.plan
  const doneAt = new Map<string, number>()
  for (let i = 0; i <= latest; i++) {
    for (const t of todosOf(messages[i]) ?? []) {
      if (t.status === 'completed' && !doneAt.has(t.content)) doneAt.set(t.content, messages[i].timestamp)
    }
  }
  const plan = { todos: todosOf(messages[latest]) ?? [], doneAt }
  cache = { src: messages[latest], plan }
  return plan
}

const time = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })

/** The agent's plan as a timeline — done (with when), current, still to
 * come — floating over the transcript, collapsed to a progress pill until
 * opened.
 *
 * Memoized: MessageList re-renders per streamed token, and this widget sits
 * inside it — with zero props, `memo` lets it bail out of all of those.
 * `messages` shows another conversation's todos (an agent's DM) instead of
 * the main chat's. */
export const TodoPanel = memo(function TodoPanel({ messages }: { messages?: AgentMessage[] } = {}) {
  const [expanded, setExpanded] = useState(false)
  const fromChat = useAgentStore((s) => (messages ? EMPTY : latestPlan(s.messages)))
  const { todos, doneAt } = messages ? latestPlan(messages) : fromChat
  const doneCount = todos.filter((t) => t.status === 'completed').length

  if (todos.length === 0) return null

  return (
    <Box sx={{ position: 'absolute', top: 10, right: 10, zIndex: 20, width: 272, userSelect: 'none' }}>
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
          }}
        >
          <TaskLinear size={13} color="currentColor" />
          <Typography component="span" variant="caption" sx={{ fontWeight: 600, color: 'text.primary' }}>
            {doneCount}/{todos.length}
          </Typography>
          <Typography component="span" variant="caption">
            done
          </Typography>
          {/* Progress at a glance, without opening the panel. */}
          <Box sx={{ flex: 1, height: 3, borderRadius: 2, bgcolor: 'action.hover', overflow: 'hidden', mx: 0.5 }}>
            <Box sx={{ width: `${(doneCount / todos.length) * 100}%`, height: '100%', bgcolor: 'success.main', transition: 'width 0.3s' }} />
          </Box>
          <Box sx={{ display: 'flex' }}>
            {expanded ? <ArrowUpLinear size={10} color="currentColor" /> : <ArrowDownLinear size={10} color="currentColor" />}
          </Box>
        </Stack>

        {expanded && (
          <Box sx={{ borderTop: 1, borderColor: 'divider', px: 1.5, py: 1.25, maxHeight: 320, overflowY: 'auto' }}>
            {todos.map((t, i) => {
              const last = i === todos.length - 1
              const done = t.status === 'completed'
              const current = t.status === 'in_progress'
              return (
                <Stack key={i} direction="row" sx={{ gap: 1.25, alignItems: 'stretch' }}>
                  {/* The rail: a marker per item, joined by a line. */}
                  <Stack sx={{ alignItems: 'center', width: 14, flexShrink: 0 }}>
                    <Box sx={{ display: 'flex', mt: 0.25, color: done ? 'success.main' : current ? 'primary.main' : 'text.disabled' }}>
                      {done ? (
                        <TickCircleLinear size={14} color="currentColor" />
                      ) : t.status === 'cancelled' ? (
                        <StopCircleLinear size={14} color="currentColor" />
                      ) : (
                        <Box
                          sx={{
                            width: 9,
                            height: 9,
                            m: '2.5px',
                            borderRadius: '50%',
                            border: 1.5,
                            borderColor: 'currentColor',
                            bgcolor: current ? 'currentColor' : 'transparent',
                          }}
                        />
                      )}
                    </Box>
                    {!last && <Box sx={{ flex: 1, width: '1px', bgcolor: done ? 'success.main' : 'divider', opacity: done ? 0.5 : 1, my: 0.25 }} />}
                  </Stack>
                  <Box sx={{ pb: last ? 0 : 1, minWidth: 0 }}>
                    <Typography
                      variant="caption"
                      sx={{
                        display: 'block',
                        lineHeight: 1.45,
                        color: done || t.status === 'cancelled' ? 'text.secondary' : current ? 'text.primary' : 'text.secondary',
                        fontWeight: current ? 600 : 400,
                        textDecoration: t.status === 'cancelled' ? 'line-through' : 'none',
                      }}
                    >
                      {t.content}
                    </Typography>
                    {(done && doneAt.has(t.content)) || current ? (
                      <Typography variant="caption" color="text.disabled" sx={{ display: 'block', fontSize: '0.6875rem' }}>
                        {current ? 'In progress' : `Done ${time(doneAt.get(t.content)!)}`}
                        {done && t.summary ? ` — ${t.summary}` : ''}
                      </Typography>
                    ) : null}
                  </Box>
                </Stack>
              )
            })}
          </Box>
        )}
      </Box>
    </Box>
  )
})
