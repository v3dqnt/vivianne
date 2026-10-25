import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, Stack, Typography, alpha, useTheme } from '../../../ui'
import { Markdown } from '../../ai/components/Markdown'
import { ReportMarkdown } from '../../ai/components/ReportMarkdown'
import { ChatComposer, AttachmentChip } from '../../ai/components/ChatComposer'
import type { TeamAgent } from '../store/teamStore'
import { useBotsStore, type BotMessage, type DmAttachments, type PendingApproval } from '../store/botsStore'
import { Avatar, LiveAvatar, colorOf, faceOf } from './bits'
import { LiveRunCard, StepsToggle } from './BotSteps'
import { MentionShell, MentionRow } from '../../ai/components/MentionMenu'
import { FollowUpContext } from '../../ai/components/InteractiveBlock'
import { MessageList } from '../../ai/components/MessageList'
import { TodoPanel } from '../../ai/components/TodoPanel'
import { isPlanNoteText, PlanNoteDivider } from '../../ai/components/HandoffDivider'
import type { AgentMessage } from '../../ai/lib/types'

const KIND_LABEL: Partial<Record<BotMessage['kind'], string>> = {
  needs_you: 'Needs you',
  routine: 'Routine',
  checkin: 'Check-in',
}

/** When the transcript's newest message was written. */
function lastTs(t: AgentMessage[]): number {
  for (let i = t.length - 1; i >= 0; i--) {
    const ts = (t[i] as { timestamp?: number }).timestamp
    if (typeof ts === 'number') return ts
  }
  return 0
}

function time(ts: number): string {
  const d = new Date(ts)
  const today = new Date().toDateString() === d.toDateString()
  return today
    ? d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) + ' ' + d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

function Message({ m, team, prev, root }: { m: BotMessage; team: TeamAgent[]; prev: BotMessage | undefined; root: string }) {
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
  // A plan approval or plan comments, sent by the app for the user: a marker, not the XML.
  if (m.from === 'user' && isPlanNoteText(m.text)) return <PlanNoteDivider text={m.text} />
  if (m.from === 'user') {
    return (
      <Stack sx={{ alignItems: 'flex-end', mt: grouped ? 0.25 : 1.25 }}>
        {m.attachments && m.attachments.length > 0 && (
          <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap', justifyContent: 'flex-end', mb: 0.5, maxWidth: '80%' }}>
            {m.attachments.map((a, i) => (
              <AttachmentChip key={`${a.name}${i}`} label={a.name} kind={a.kind} />
            ))}
          </Stack>
        )}
        {m.text !== '(attachments)' && (
          <Box sx={{ maxWidth: '80%', px: 1.75, py: 1.125, borderRadius: '18px', bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === 'dark' ? 0.09 : 0.06), fontSize: '0.875rem' }}>
            <Markdown text={m.text} />
          </Box>
        )}
      </Stack>
    )
  }
  const color = bot ? colorOf(bot) : theme.palette.text.secondary
  const label = KIND_LABEL[m.kind]
  const needs = m.kind === 'needs_you'
  return (
    <Stack direction="row" sx={{ gap: 1.25, mt: grouped ? 0.25 : 1.5, alignItems: 'flex-start' }}>
      <Box sx={{ width: 28, flexShrink: 0 }}>
        {!grouped && (bot ? <LiveAvatar root={root} agent={bot} size={28} /> : <Avatar name={m.from} color={color} size={28} />)}
      </Box>
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
          {needs ? <Markdown text={m.text} /> : <ReportMarkdown text={m.text} />}
        </Box>
        {m.steps && m.steps.length > 0 && <StepsToggle steps={m.steps} />}
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
        {bot && <Avatar {...faceOf(bot)} size={22} />}
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
  root,
  channel,
  messages,
  team,
  approvals,
  onSend,
  placeholder,
  allowEveryone,
  empty,
  attachments = false,
  toolbar,
  transcript,
  onRevertTo,
}: {
  /** The project, and which conversation this is (`room` or a bot id): runs
   * replying here show live, step by step. */
  root: string
  channel: string
  messages: BotMessage[]
  team: TeamAgent[]
  approvals: PendingApproval[]
  onSend: (text: string, attachments?: DmAttachments) => Promise<void>
  placeholder: string
  allowEveryone: boolean
  empty?: React.ReactNode
  /** Accept files and images (attach button, paste, drop). */
  attachments?: boolean
  /** Controls at the composer's left edge (a DM's model picker). */
  toolbar?: React.ReactNode
  /** A DM's full working transcript. When there is one, the conversation is
   * shown the way the main chat shows it: every tool call with its result,
   * files and previews inline, instead of replies with folded steps. */
  transcript?: AgentMessage[] | null
  /** Go back to just before transcript message `index` (files included). */
  onRevertTo?: (index: number) => Promise<void>
}) {
  const theme = useTheme()
  const [text, setText] = useState('')
  const [pick, setPick] = useState(0)
  const scroller = useRef<HTMLDivElement | null>(null)
  const liveAll = useBotsStore((s) => s.live)
  const status = useBotsStore((s) => s.status)
  const running = useMemo(
    () =>
      team
        .map((agent) => ({ agent, run: liveAll[`${root}::${agent.id}`] }))
        .filter(({ agent, run }) => run && run.channel === channel && status[`${root}::${agent.id}`]?.state === 'working'),
    [team, liveAll, status, root, channel],
  )
  const stepCount = running.reduce((n, r) => n + (r.run?.steps.length ?? 0), 0)

  // Follow the conversation when new messages arrive near the bottom.
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el) return
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 240) el.scrollTop = el.scrollHeight
  }, [messages.length, approvals.length, running.length, stepCount])
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

  const onMentionKey = (e: React.KeyboardEvent) => {
    if (options.length === 0) return false
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const n = options.length
      setPick((p) => (e.key === 'ArrowDown' ? (p + 1) % n : (p - 1 + n) % n))
      return true
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      complete(options[pick].id)
      return true
    }
    return false
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0, position: 'relative' }}>
      {/* The agent's todo list, pinned over its conversation like the main chat's. */}
      {transcript && transcript.length > 0 && <TodoPanel messages={transcript} />}
      <Box ref={scroller} sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 2, py: 1.5 }}>
        <Box sx={{ maxWidth: 820, mx: 'auto' }}>
          {messages.length === 0 && approvals.length === 0 && !transcript?.length ? empty : null}
          {/* A button in a summary card answers in this conversation, not the main chat. */}
          <FollowUpContext.Provider value={(t) => void onSend(t)}>
            {transcript && transcript.length > 0 ? (
              <>
                <MessageList
                  messages={transcript}
                  readOnly
                  emptyText=""
                  onRevertTo={onRevertTo}
                  revertNote="Files it changed after this point are put back as they were."
                />
                {/* Since the transcript was last saved: what you sent while the turn
                    runs, and notes such as a failed model call. The agent's own
                    replies are already in the transcript. */}
                {messages
                  .filter((m) => (m.from === 'user' || m.from === 'system') && m.ts > lastTs(transcript))
                  .map((m) => (
                    <Message key={m.id} m={m} team={team} prev={undefined} root={root} />
                  ))}
              </>
            ) : (
              messages.map((m, i) => <Message key={m.id} m={m} team={team} prev={messages[i - 1]} root={root} />)
            )}
          </FollowUpContext.Provider>
          {running.map(({ agent, run }) => (
            <LiveRunCard key={agent.id} agent={agent} steps={run!.steps} />
          ))}
          {approvals.map((a) => (
            <ApprovalCard key={a.requestId} a={a} team={team} />
          ))}
        </Box>
      </Box>
      <Box sx={{ px: 2, pb: 1.5, pt: 0.5 }}>
        <Box sx={{ maxWidth: 820, mx: 'auto' }}>
          <ChatComposer
            value={text}
            onChange={(t) => {
              setText(t)
              setPick(0)
            }}
            onSubmit={(t, a) => onSend(t.trim(), a.images.length + a.files.length > 0 ? a : undefined)}
            placeholder={placeholder}
            attachments={attachments}
            onKeyDownFirst={onMentionKey}
            left={toolbar}
            overlay={
              options.length > 0 && (
                <MentionShell title="Mention a teammate" width={360}>
                  <Box sx={{ px: 0.5, pb: 0.5 }}>
                    {options.map((o, i) => (
                      <MentionRow
                        key={o.id}
                        active={i === pick}
                        onSelect={() => complete(o.id)}
                        onHover={() => setPick(i)}
                        tint={o.agent ? colorOf(o.agent) : theme.palette.primary.main}
                        icon={o.agent ? <Avatar {...faceOf(o.agent)} size={20} /> : <Box component="span" sx={{ fontSize: '0.75rem', fontWeight: 700 }}>@</Box>}
                        title={o.label}
                        subtitle={o.agent ? (o.agent.role || `@${o.id}`) : 'Everyone on the team'}
                        tag={<Box component="span" sx={{ fontSize: '0.625rem', color: 'text.disabled', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>@{o.id}</Box>}
                      />
                    ))}
                  </Box>
                </MentionShell>
              )
            }
          />
        </Box>
      </Box>
    </Stack>
  )
}
