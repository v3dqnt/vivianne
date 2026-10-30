import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography, alpha, keyframes, useTheme } from '../../../ui'
import { ArrowUpLinear, ChevronRightLinear, StopLinear } from '../../../icons'
import { useAgentStore } from '../../ai/store/agentStore'
import { Markdown } from '../../ai/components/Markdown'
import { ImageBlock } from '../../ai/components/ImageBlock'
import { FollowUpContext } from '../../ai/components/InteractiveBlock'
import { AskQuestionDialog } from '../../ai/components/AskQuestionDialog'
import { ConfirmationDialog } from '../../ai/components/ConfirmationDialog'
import { summarizeTools } from '../../ai/lib/toolGroups'
import type { TeamAgent } from '../store/teamStore'
import { composerButton, toItems, type Item, type ToolResult } from '../lib/messenger'
import { describeApproval, type ApprovalWidget as Widget } from '../lib/widgets'
import type { WidgetState } from '../lib/widgetLab'
import { useBotsStore, botKey } from '../store/botsStore'
import { ApprovalWidget } from './ApprovalWidget'
import { call } from '../../../ipc'
import { Avatar, colorOf, faceOf } from './bits'
import { useBotFace } from './AgentFace'
import { ErrorsButton } from '../../ai/components/ErrorsButton'
import { EditLinear } from '../../../icons'

/** Blocks that need room: drawn at full width, not squeezed into a bubble. */
const WIDE = /```(ui|chart|widget|mermaid|plot|model3d|mol3d|chem|bio|physics)\b/

/** `@id` / `@Name` of a teammate, as a link Markdown draws: the chip style
 * below picks it out (`a[href^="#agent:"]`) in that agent's colour. */
function highlightMentions(text: string, team: TeamAgent[]): string {
  if (!team.length || !text.includes('@')) return text
  return text.replace(/(^|[\s(])@([\w-]+)/g, (all, pre: string, tag: string) => {
    const a = team.find((x) => x.id.toLowerCase() === tag.toLowerCase() || x.name.replace(/\s+/g, '').toLowerCase() === tag.toLowerCase())
    return a ? `${pre}[@${a.name}](#agent:${a.id})` : all
  })
}

const clock = (ts: number) => new Date(ts).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })

const bounce = keyframes({ '0%, 80%, 100%': { transform: 'translateY(0)', opacity: 0.45 }, '40%': { transform: 'translateY(-4px)', opacity: 1 } })

function Typing({ color }: { color: string }) {
  return (
    <Stack direction="row" sx={{ gap: '4px', px: '16px', py: '12px', borderRadius: '20px', borderBottomLeftRadius: '6px', bgcolor: 'action.hover', width: 'fit-content' }}>
      {[0, 1, 2].map((i) => (
        <Box key={i} sx={{ width: 7, height: 7, borderRadius: '50%', bgcolor: color, animation: `${bounce} 1.2s ${i * 0.15}s infinite ease-in-out` }} />
      ))}
    </Stack>
  )
}

/** The agent's tool work between two messages, as one quiet line that opens
 * onto the list of what it did. */
function Activity({ tools, color }: { tools: ToolResult[]; color: string }) {
  const [open, setOpen] = useState(false)
  const failed = tools.some((t) => t.isError)
  const summary = summarizeTools(tools) || `Used ${tools.length} tool${tools.length === 1 ? '' : 's'}`
  return (
    <Stack sx={{ alignItems: 'flex-start', pl: '44px', my: '4px' }}>
      <ButtonBase
        onClick={() => setOpen((v) => !v)}
        sx={{ gap: '6px', px: '10px', height: 26, borderRadius: '999px', fontSize: '12px', color: failed ? 'error.main' : 'text.secondary', bgcolor: alpha(color, 0.1), '&:hover': { bgcolor: alpha(color, 0.18), color: 'text.primary' } }}
      >
        <Box component="span" sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: failed ? 'error.main' : color }} />
        {summary}
        <Box component="span" sx={{ display: 'flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 0.15s' }}>
          <ChevronRightLinear size={11} color="currentColor" />
        </Box>
      </ButtonBase>
      {open && (
        <Stack sx={{ mt: '6px', gap: '2px', pl: '12px', borderLeft: 2, borderColor: alpha(color, 0.35) }}>
          {tools.map((t) => {
            const a = (t.arguments ?? {}) as Record<string, unknown>
            const target = String(a.path ?? a.url ?? a.query ?? a.command ?? a.action ?? '')
            return (
              <Typography key={t.toolCallId} sx={{ fontSize: '12px', color: t.isError ? 'error.main' : 'text.secondary', lineHeight: '20px' }} noWrap>
                {t.toolName}
                {target && (
                  <Box component="span" sx={{ color: 'text.disabled', ml: '6px', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '11px' }}>
                    {target.slice(0, 80)}
                  </Box>
                )}
              </Typography>
            )
          })}
        </Stack>
      )}
    </Stack>
  )
}

function Bubble({
  item,
  agent: self,
  team,
  color,
  first,
  last,
}: {
  item: Extract<Item, { kind: 'bubble' }>
  agent: TeamAgent
  team: TeamAgent[]
  color: string
  first: boolean
  last: boolean
}) {
  const other = item.by ? team.find((a) => a.id === item.by) : undefined
  const agent = other ?? self
  const theme = useTheme()
  const mine = item.from === 'you'
  const wide = WIDE.test(item.text)
  // Corners tighten where bubbles from the same side meet, like a messenger.
  const r = '20px'
  const join = '6px'
  const radius = mine
    ? { borderRadius: r, borderTopRightRadius: first ? r : join, borderBottomRightRadius: last ? r : join }
    : { borderRadius: r, borderTopLeftRadius: first ? r : join, borderBottomLeftRadius: last ? r : join }
  const onColor = theme.palette.getContrastText(color)
  return (
    <Stack direction="row" sx={{ justifyContent: mine ? 'flex-end' : 'flex-start', alignItems: 'flex-end', gap: '8px', mt: first ? '12px' : '3px' }}>
      {!mine && <Box sx={{ width: 36, flexShrink: 0 }}>{last && <Avatar {...faceOf(agent)} size={36} />}</Box>}
      <Stack sx={{ maxWidth: wide ? 'min(100%, 680px)' : '72%', width: wide ? '100%' : undefined, alignItems: mine ? 'flex-end' : 'flex-start', minWidth: 0 }}>
        {item.images.length > 0 && (
          <Stack direction="row" sx={{ gap: '6px', flexWrap: 'wrap', mb: item.text ? '4px' : 0, justifyContent: mine ? 'flex-end' : 'flex-start' }}>
            {item.images.map((img, i) => (
              <Box key={i} sx={{ borderRadius: '16px', overflow: 'hidden', maxWidth: 280 }}>
                <ImageBlock block={img} />
              </Box>
            ))}
          </Stack>
        )}
        {other && first && <Typography sx={{ fontSize: '11px', fontWeight: 600, color: colorOf(other), mb: '3px', px: '6px' }}>{other.name}</Typography>}
        {item.text && (
          <Box
            sx={{
              ...radius,
              ...(other && { boxShadow: `inset 3px 0 0 ${colorOf(other)}` }),
              px: '14px',
              py: '9px',
              minWidth: 0,
              maxWidth: '100%',
              ...(mine
                ? {
                    background: `linear-gradient(135deg, ${color}, ${alpha(color, 0.78)})`,
                    color: onColor,
                    '& a, & strong, & h1, & h2, & h3': { color: onColor },
                    // Code and file chips on the coloured bubble: its own ink on a light veil.
                    '& code, & [role=link]': { color: `${onColor} !important`, bgcolor: `${alpha(onColor, 0.16)} !important` },
                  }
                : { bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === 'dark' ? 0.08 : 0.05) }),
              // A message's own text: tighter than a document's.
              '& p:last-child, & ul:last-child, & ol:last-child': { mb: 0 },
              // A mentioned agent: a chip in its colour (not a link to follow).
              '& a[href^="#agent:"]': { pointerEvents: 'none', textDecoration: 'none', fontWeight: 600, px: '5px', py: '1px', borderRadius: '6px' },
              ...Object.fromEntries(
                team.map((a) => [`& a[href="#agent:${a.id}"]`, mine ? { color: `${onColor} !important`, bgcolor: alpha(onColor, 0.2) } : { color: `${colorOf(a)} !important`, bgcolor: alpha(colorOf(a), 0.16) }]),
              ),
            }}
          >
            <Markdown text={highlightMentions(item.text, team)} />
          </Box>
        )}
        {last && (
          <Typography sx={{ fontSize: '11px', color: 'text.disabled', mt: '4px', px: '6px' }}>{clock(item.ts)}</Typography>
        )}
      </Stack>
    </Stack>
  )
}

/**
 * A 24/7 agent's conversation, as a messenger: your messages on the right in
 * the agent's colour, its replies on the left with its face, the tool work
 * it did folded into one line between them, and dots while it is typing.
 * Built for talking to someone, not for reading a coding session.
 */
export function AgentMessenger({ root, agent, team = [], status, onEdit }: { root: string; agent: TeamAgent; team?: TeamAgent[]; status: string; onEdit?: () => void }) {
  const theme = useTheme()
  const color = colorOf(agent)
  const messages = useAgentStore((s) => s.messages)
  const isBusy = useAgentStore((s) => s.isBusy)
  const sendMessage = useAgentStore((s) => s.sendMessage)
  const abort = useAgentStore((s) => s.abort)
  const pendingPermission = useAgentStore((s) => s.pendingPermission)
  const respondPermission = useAgentStore((s) => s.respondPermission)
  const face = useBotFace(root, agent.id)
  // Working from anywhere: this chat, a routine, a check-in, another agent.
  const agentState = useBotsStore((s) => s.status[botKey(root, agent.id)]?.state ?? 'idle')
  const allApprovals = useBotsStore((s) => s.approvals)
  const respondApproval = useBotsStore((s) => s.respond)
  const busy = isBusy || agentState === 'working'
  // What it is waiting on you for, as cards: its rules' approvals, and a
  // permission its model asked for. Answered ones stay, stamped.
  const asks = useMemo(() => {
    const out: { id: string; widget: Widget; answer: (yes: boolean) => Promise<void> }[] = allApprovals
      .filter((a) => a.root === root && a.botId === agent.id)
      .map((a) => ({ id: a.requestId, widget: describeApproval(a, agent.name), answer: (yes: boolean) => respondApproval(a.requestId, yes) }))
    if (pendingPermission && !out.some((o) => o.id === pendingPermission.requestId)) {
      out.push({
        id: pendingPermission.requestId,
        widget: describeApproval({ toolName: pendingPermission.toolName, args: (pendingPermission.args ?? {}) as Record<string, unknown>, reason: pendingPermission.reason ?? null }, agent.name),
        answer: (yes: boolean) => respondPermission(yes),
      })
    }
    return out
  }, [allApprovals, pendingPermission, root, agent.id, agent.name, respondApproval, respondPermission])
  const [answered, setAnswered] = useState<{ id: string; widget: Widget; state: WidgetState }[]>([])
  useEffect(() => setAnswered([]), [agent.id])
  const answer = (ask: (typeof asks)[number]) => async (yes: boolean) => {
    await ask.answer(yes)
    setAnswered((a) => [...a, { id: ask.id, widget: ask.widget, state: yes ? 'approved' : 'denied' }])
  }
  const stop = () => {
    void call('cmd_bots_stop', { root, botId: agent.id })
    void abort()
  }
  const [text, setText] = useState('')
  const [pick, setPick] = useState(0)
  const scroller = useRef<HTMLDivElement | null>(null)
  // `@par` at the caret: the other agents it could be.
  const query = /(^|\s)@([\w-]*)$/.exec(text)?.[2]
  const options = useMemo(() => {
    if (query === undefined) return []
    const q = query.toLowerCase()
    return team.filter((a) => a.id !== agent.id && (a.id.toLowerCase().startsWith(q) || a.name.toLowerCase().startsWith(q))).slice(0, 6)
  }, [query, team, agent.id])
  const complete = (id: string) => {
    setText((t) => t.replace(/@([\w-]*)$/, `@${id} `))
    setPick(0)
  }
  const items = useMemo(() => toItems(messages), [messages])

  // Stay at the newest message while you are near it.
  useLayoutEffect(() => {
    const el = scroller.current
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 280) el.scrollTop = el.scrollHeight
  }, [items, busy, asks.length])
  useEffect(() => {
    const el = scroller.current
    if (el) el.scrollTop = el.scrollHeight
  }, [agent.id])

  const send = () => {
    const t = text.trim()
    if (!t) return
    setText('')
    void sendMessage(t)
  }

  const lastBubble = [...items].reverse().find((i) => i.kind === 'bubble')
  const typing = busy && asks.length === 0 && !(lastBubble?.kind === 'bubble' && lastBubble.from === 'agent' && items[items.length - 1] === lastBubble)

  return (
    <Stack
      sx={{
        flex: 1,
        minHeight: 0,
        position: 'relative',
        // The agent's colour washes the conversation, faintly.
        background: `radial-gradient(120% 60% at 0% 0%, ${alpha(color, 0.14)}, transparent 60%), radial-gradient(90% 50% at 100% 100%, ${alpha(color, 0.1)}, transparent 60%)`,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', px: '20px', py: '14px', flexShrink: 0 }}>
        <Avatar {...faceOf(agent)} size={44} state={face} animate />
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography sx={{ fontSize: '16px', fontWeight: 600, lineHeight: 1.25 }} noWrap>
            {agent.name}
          </Typography>
          <Typography sx={{ fontSize: '12px', color: isBusy ? color : 'text.secondary' }} noWrap>
            {status}
          </Typography>
        </Stack>
        {onEdit && (
          <ButtonBase onClick={onEdit} aria-label={`Edit ${agent.name}`} title="Name, look, job, model and instructions" sx={{ width: 32, height: 32, borderRadius: '50%', color: 'text.secondary', '&:hover': { bgcolor: alpha(color, 0.14), color: 'text.primary' } }}>
            <EditLinear size={16} color="currentColor" />
          </ButtonBase>
        )}
      </Stack>

      <Box ref={scroller} sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '20px', pb: '12px' }}>
        <Box sx={{ maxWidth: 760, mx: 'auto' }}>
          {items.length === 0 && !isBusy && (
            <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: '10px', py: '72px' }}>
              <Avatar {...faceOf(agent)} size={72} state={face} animate />
              <Typography sx={{ fontSize: '18px', fontWeight: 600 }}>Say hi to {agent.name}</Typography>
              <Typography sx={{ fontSize: '13px', color: 'text.secondary', maxWidth: 360 }}>{agent.role || 'Ask it anything, or give it something to look after.'}</Typography>
            </Stack>
          )}
          <FollowUpContext.Provider value={(t) => void sendMessage(t)}>
            {items.map((item, i) => {
              if (item.kind === 'day')
                return (
                  <Typography key={item.key} sx={{ textAlign: 'center', fontSize: '11px', fontWeight: 600, color: 'text.disabled', mt: '20px', mb: '4px' }}>
                    {item.label}
                  </Typography>
                )
              if (item.kind === 'note')
                return (
                  <Typography key={item.key} sx={{ textAlign: 'center', fontSize: '12px', color: 'text.secondary', my: '8px' }}>
                    {item.text}
                  </Typography>
                )
              if (item.kind === 'activity') return <Activity key={item.key} tools={item.tools} color={color} />
              const prev = items[i - 1]
              const next = items[i + 1]
              const same = (o: Item | undefined) => o?.kind === 'bubble' && o.from === item.from && o.by === item.by
              return <Bubble key={item.key} item={item} agent={agent} team={team} color={color} first={!same(prev)} last={!same(next)} />
            })}
          </FollowUpContext.Provider>
          {[...answered.map((a) => ({ ...a, live: null as null | (typeof asks)[number] })), ...asks.filter((a) => !answered.some((x) => x.id === a.id)).map((a) => ({ id: a.id, widget: a.widget, state: 'pending' as WidgetState, live: a }))].map((c) => (
            <Stack key={c.id} direction="row" sx={{ alignItems: 'flex-end', gap: '8px', mt: '12px' }}>
              <Avatar {...faceOf(agent)} size={36} state={face} animate />
              <ApprovalWidget widget={c.widget} state={c.state} accent={color} onAnswer={c.live ? answer(c.live) : undefined} />
            </Stack>
          ))}
          {typing && (
            <Stack direction="row" sx={{ alignItems: 'flex-end', gap: '8px', mt: '12px' }}>
              <Avatar {...faceOf(agent)} size={36} state={face} animate />
              <Typing color={color} />
            </Stack>
          )}
        </Box>
      </Box>

      <Box sx={{ px: '20px', pb: '16px', pt: '4px', position: 'relative' }}>
        {options.length > 0 && (
          <Stack sx={{ position: 'absolute', bottom: 'calc(100% + 4px)', left: '50%', transform: 'translateX(-50%)', width: 'min(360px, calc(100% - 40px))', p: '6px', borderRadius: '16px', bgcolor: 'background.paper', border: 1, borderColor: 'divider', boxShadow: theme.elevation.lg, zIndex: 2 }}>
            {options.map((o, i) => (
              <ButtonBase
                key={o.id}
                onMouseDown={(e) => {
                  e.preventDefault()
                  complete(o.id)
                }}
                onMouseEnter={() => setPick(i)}
                sx={{ justifyContent: 'flex-start', gap: '10px', px: '10px', py: '8px', borderRadius: '10px', bgcolor: i === pick ? alpha(colorOf(o), 0.16) : 'transparent' }}
              >
                <Avatar {...faceOf(o)} size={26} />
                <Stack sx={{ minWidth: 0, textAlign: 'left' }}>
                  <Typography sx={{ fontSize: '13px', fontWeight: 600 }} noWrap>
                    {o.name}
                  </Typography>
                  <Typography sx={{ fontSize: '11px', color: 'text.secondary' }} noWrap>
                    @{o.id}
                    {o.role ? ` · ${o.role}` : ''}
                  </Typography>
                </Stack>
              </ButtonBase>
            ))}
          </Stack>
        )}
        <Stack
          direction="row"
          sx={{
            maxWidth: 760,
            mx: 'auto',
            alignItems: 'flex-end',
            gap: '8px',
            pl: '18px',
            pr: '6px',
            py: '6px',
            borderRadius: '24px',
            bgcolor: 'background.paper',
            border: 1,
            borderColor: alpha(color, 0.35),
            boxShadow: `0 8px 28px ${alpha(color, theme.palette.mode === 'dark' ? 0.18 : 0.12)}`,
            '&:focus-within': { borderColor: color },
          }}
        >
          <InputBase
            multiline
            maxRows={6}
            value={text}
            onChange={(e) => {
              setText(e.target.value)
              setPick(0)
            }}
            onKeyDown={(e) => {
              if (options.length && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
                e.preventDefault()
                setPick((p) => (e.key === 'ArrowDown' ? (p + 1) % options.length : (p - 1 + options.length) % options.length))
                return
              }
              if (options.length && (e.key === 'Enter' || e.key === 'Tab')) {
                e.preventDefault()
                complete(options[pick].id)
                return
              }
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                send()
              }
            }}
            placeholder={team.length > 1 ? `Message ${agent.name}, or @ another agent` : `Message ${agent.name}`}
            sx={{ flex: 1, fontSize: '14px', py: '7px' }}
          />
          <ErrorsButton sx={{ height: 36, borderRadius: '18px' }} />
          {composerButton({ chatBusy: isBusy, agentState, typed: text }) === 'stop' ? (
            <ButtonBase onClick={stop} aria-label="Stop" sx={{ width: 36, height: 36, borderRadius: '50%', bgcolor: alpha(color, 0.16), color }}>
              <StopLinear size={16} color="currentColor" />
            </ButtonBase>
          ) : (
            <ButtonBase
              onClick={send}
              disabled={!text.trim()}
              aria-label="Send"
              sx={{ width: 36, height: 36, borderRadius: '50%', bgcolor: color, color: theme.palette.getContrastText(color), transition: 'transform 0.12s, opacity 0.12s', '&:disabled': { opacity: 0.35 }, '&:active': { transform: 'scale(0.94)' } }}
            >
              <ArrowUpLinear size={18} color="currentColor" />
            </ButtonBase>
          )}
        </Stack>
      </Box>

      <AskQuestionDialog />
      <ConfirmationDialog />
    </Stack>
  )
}
