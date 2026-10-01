import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography, alpha, useTheme } from '../../../ui'
import { SendLinear } from '../../../icons'
import { Markdown } from '../../ai/components/Markdown'
import type { TeamAgent } from '../store/teamStore'
import { useBotsStore, type BotMessage, type PendingApproval } from '../store/botsStore'
import { Avatar, colorOf } from './bits'

const KIND_LABEL: Partial<Record<BotMessage['kind'], string>> = {
  needs_you: 'Needs you',
  routine: 'Routine',
  checkin: 'Check-in',
}

function time(ts: number): string {
  const d = new Date(ts)
  const today = new Date().toDateString() === d.toDateString()
  return today
    ? d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) + ' ' + d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

function Message({ m, team, prev }: { m: BotMessage; team: TeamAgent[]; prev: BotMessage | undefined }) {
  const theme = useTheme()
  const bot = team.find((a) => a.id === m.from)
  // Consecutive posts by the same author within 5 minutes read as one.
  const grouped = prev && prev.from === m.from && m.ts - prev.ts < 5 * 60_000 && prev.kind === m.kind
  if (m.from === 'system') {
    return (
      <Typography variant="caption" color="text.secondary" sx={{ display: 'block', textAlign: 'center', py: 0.5, fontSize: '0.6875rem' }}>
        {m.text}
      </Typography>
    )
  }
  if (m.from === 'user') {
    return (
      <Stack sx={{ alignItems: 'flex-end', mt: grouped ? 0.25 : 1.25 }}>
        <Box sx={{ maxWidth: '80%', px: 1.5, py: 1, borderRadius: '14px', bgcolor: alpha(theme.palette.text.primary, 0.07), fontSize: '0.875rem' }}>
          <Markdown text={m.text} />
        </Box>
      </Stack>
    )
  }
  const color = bot ? colorOf(bot) : theme.palette.text.secondary
  const label = KIND_LABEL[m.kind]
  const needs = m.kind === 'needs_you'
  return (
    <Stack direction="row" sx={{ gap: 1.25, mt: grouped ? 0.25 : 1.5, alignItems: 'flex-start' }}>
      <Box sx={{ width: 28, flexShrink: 0 }}>{!grouped && <Avatar name={bot?.name ?? m.from} color={color} size={28} />}</Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        {!grouped && (
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75 }}>
            <Typography variant="caption" sx={{ fontWeight: 700, color }}>
              {bot?.name ?? m.from}
            </Typography>
            {label && (
              <Typography
                variant="caption"
                sx={{
                  fontSize: '0.625rem',
                  fontWeight: 700,
                  textTransform: 'uppercase',
                  letterSpacing: 0.4,
                  color: needs ? 'warning.main' : 'text.secondary',
                }}
              >
                {label}
              </Typography>
            )}
            <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
              {time(m.ts)}
            </Typography>
          </Stack>
        )}
        <Box
          sx={{
            fontSize: '0.875rem',
            ...(needs ? { mt: 0.5, p: 1.25, borderRadius: '10px', border: 1, borderColor: alpha(theme.palette.warning.main, 0.4), bgcolor: alpha(theme.palette.warning.main, 0.06) } : {}),
          }}
        >
          <Markdown text={m.text} />
        </Box>
      </Stack>
    </Stack>
  )
}

function ApprovalCard({ a, team }: { a: PendingApproval; team: TeamAgent[] }) {
  const theme = useTheme()
  const respond = useBotsStore((s) => s.respond)
  const bot = team.find((x) => x.id === a.botId)
  const detail = String(a.args.command ?? a.args.path ?? a.args.action ?? a.args.url ?? a.args.query ?? '')
  return (
    <Stack sx={{ gap: 1, p: 1.25, my: 1, borderRadius: '12px', border: 1, borderColor: alpha(theme.palette.warning.main, 0.45), bgcolor: alpha(theme.palette.warning.main, 0.06) }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        {bot && <Avatar name={bot.name} color={colorOf(bot)} size={22} />}
        <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }}>
          {bot?.name ?? a.botId} wants to use <code>{a.toolName}</code>
        </Typography>
      </Stack>
      {detail && (
        <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', wordBreak: 'break-all' }}>
          {detail.slice(0, 300)}
        </Typography>
      )}
      {a.reason && (
        <Typography variant="caption" color="text.secondary">
          {a.reason}
        </Typography>
      )}
      <Stack direction="row" sx={{ gap: 1 }}>
        <ButtonBase onClick={() => void respond(a.requestId, true)} sx={{ px: 1.5, height: 28, borderRadius: '8px', bgcolor: 'text.primary', color: 'background.paper', fontSize: '0.75rem', fontWeight: 600 }}>
          Allow
        </ButtonBase>
        <ButtonBase onClick={() => void respond(a.requestId, false)} sx={{ px: 1.5, height: 28, borderRadius: '8px', border: 1, borderColor: 'divider', fontSize: '0.75rem', fontWeight: 600 }}>
          Deny
        </ButtonBase>
      </Stack>
    </Stack>
  )
}

/**
 * A team conversation — the room, or one bot's DM. Messages read like a
 * group chat; the composer completes `@mentions` (a bot, or `@everyone` in
 * the room). Approvals a bot is waiting on appear inline.
 */
export function BotChat({
  messages,
  team,
  approvals,
  onSend,
  placeholder,
  allowEveryone,
  empty,
}: {
  messages: BotMessage[]
  team: TeamAgent[]
  approvals: PendingApproval[]
  onSend: (text: string) => Promise<void>
  placeholder: string
  allowEveryone: boolean
  empty?: React.ReactNode
}) {
  const [text, setText] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [pick, setPick] = useState(0)
  const scroller = useRef<HTMLDivElement | null>(null)

  // Follow the conversation when new messages arrive near the bottom.
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el) return
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 240) el.scrollTop = el.scrollHeight
  }, [messages.length, approvals.length])
  useEffect(() => {
    const el = scroller.current
    if (el) el.scrollTop = el.scrollHeight
  }, [])

  // `@par` at the caret → matching teammates.
  const query = /(^|\s)@([\w-]*)$/.exec(text)?.[2]
  const options = useMemo(() => {
    if (query === undefined) return []
    const q = query.toLowerCase()
    const list = team.filter((a) => a.id.toLowerCase().startsWith(q) || a.name.toLowerCase().startsWith(q)).map((a) => ({ id: a.id, label: a.name, agent: a as TeamAgent | null }))
    if (allowEveryone && 'everyone'.startsWith(q)) list.push({ id: 'everyone', label: 'Everyone', agent: null })
    return list.slice(0, 6)
  }, [query, team, allowEveryone])

  const complete = (id: string) => {
    setText((t) => t.replace(/@([\w-]*)$/, `@${id} `))
    setPick(0)
  }

  const send = async () => {
    const t = text.trim()
    if (!t) return
    setError(null)
    setText('')
    try {
      await onSend(t)
    } catch (e) {
      setText(t)
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Box ref={scroller} sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 2, py: 1.5 }}>
        <Box sx={{ maxWidth: 820, mx: 'auto' }}>
          {messages.length === 0 && approvals.length === 0 ? empty : null}
          {messages.map((m, i) => (
            <Message key={m.id} m={m} team={team} prev={messages[i - 1]} />
          ))}
          {approvals.map((a) => (
            <ApprovalCard key={a.requestId} a={a} team={team} />
          ))}
        </Box>
      </Box>
      <Box sx={{ px: 2, pb: 1.5, pt: 0.5 }}>
        <Box sx={{ maxWidth: 820, mx: 'auto', position: 'relative' }}>
          {options.length > 0 && (
            <Stack sx={{ position: 'absolute', bottom: '100%', left: 0, mb: 0.75, minWidth: 220, p: 0.5, borderRadius: '10px', border: 1, borderColor: 'divider', bgcolor: 'background.paper', boxShadow: 6, zIndex: 2 }}>
              {options.map((o, i) => (
                <ButtonBase
                  key={o.id}
                  onMouseDown={(e) => {
                    e.preventDefault()
                    complete(o.id)
                  }}
                  sx={{ justifyContent: 'flex-start', gap: 1, px: 1, py: 0.625, borderRadius: '6px', bgcolor: i === pick ? 'action.selected' : 'transparent' }}
                >
                  {o.agent ? <Avatar name={o.label} color={colorOf(o.agent)} size={20} /> : <Box sx={{ width: 20 }} />}
                  <Typography variant="body2" sx={{ fontWeight: 600 }}>
                    {o.label}
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    @{o.id}
                  </Typography>
                </ButtonBase>
              ))}
            </Stack>
          )}
          <Stack direction="row" sx={{ alignItems: 'flex-end', gap: 1, px: 1.5, py: 1, borderRadius: '16px', border: 1, borderColor: 'divider', bgcolor: 'background.card' }}>
            <InputBase
              multiline
              maxRows={8}
              value={text}
              placeholder={placeholder}
              onChange={(e) => {
                setText(e.target.value)
                setPick(0)
              }}
              onKeyDown={(e) => {
                if (options.length > 0) {
                  if (e.key === 'ArrowDown') {
                    e.preventDefault()
                    setPick((p) => (p + 1) % options.length)
                    return
                  }
                  if (e.key === 'ArrowUp') {
                    e.preventDefault()
                    setPick((p) => (p - 1 + options.length) % options.length)
                    return
                  }
                  if (e.key === 'Enter' || e.key === 'Tab') {
                    e.preventDefault()
                    complete(options[pick].id)
                    return
                  }
                }
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault()
                  void send()
                }
              }}
              sx={{ flex: 1, fontSize: '0.875rem', py: 0.25 }}
            />
            <ButtonBase
              aria-label="Send"
              onClick={() => void send()}
              disabled={!text.trim()}
              sx={{ width: 30, height: 30, borderRadius: '50%', bgcolor: text.trim() ? 'text.primary' : 'action.disabledBackground', color: 'background.paper', flexShrink: 0 }}
            >
              <SendLinear size={15} color="currentColor" />
            </ButtonBase>
          </Stack>
          {error && (
            <Typography variant="caption" color="error.main" sx={{ display: 'block', mt: 0.5 }}>
              {error}
            </Typography>
          )}
        </Box>
      </Box>
    </Stack>
  )
}
