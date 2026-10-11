import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography, alpha, useTheme } from '../../../ui'
import { SendLinear, CloseLinear, DocumentTextLinear } from '../../../icons'
import { Markdown } from '../../ai/components/Markdown'
import { AttachFileButton } from '../../ai/components/AttachFileButton'
import { buildAttachments, filesFromDataTransfer } from '../../ai/lib/attachments'
import type { ImageAttachment, FileAttachment } from '../../ai/lib/types'
import type { TeamAgent } from '../store/teamStore'
import { useBotsStore, type BotMessage, type DmAttachments, type PendingApproval } from '../store/botsStore'
import { Avatar, colorOf } from './bits'
import { LiveRunCard, StepsToggle } from './BotSteps'
import { MentionShell, MentionRow } from '../../ai/components/MentionMenu'

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
        {m.attachments && m.attachments.length > 0 && (
          <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap', justifyContent: 'flex-end', mb: 0.5, maxWidth: '80%' }}>
            {m.attachments.map((a, i) => (
              <Chip key={`${a.name}${i}`} label={a.name} kind={a.kind} />
            ))}
          </Stack>
        )}
        {m.text !== '(attachments)' && (
          <Box sx={{ maxWidth: '80%', px: 1.5, py: 1, borderRadius: '14px', bgcolor: alpha(theme.palette.text.primary, 0.07), fontSize: '0.875rem' }}>
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
        {m.steps && m.steps.length > 0 && <StepsToggle steps={m.steps} />}
      </Stack>
    </Stack>
  )
}

/** An attachment, as a small chip: name and kind, with remove when pending. */
function Chip({ label, kind, image, onRemove }: { label: string; kind: 'image' | 'file'; image?: string; onRemove?: () => void }) {
  return (
    <Stack
      direction="row"
      aria-label={`${kind === 'image' ? 'Image' : 'File'}: ${label}`}
      sx={{ alignItems: 'center', gap: 0.75, pl: image ? 0.375 : 1, pr: onRemove ? 0.375 : 1, py: 0.375, borderRadius: '10px', border: 1, borderColor: 'divider', bgcolor: 'background.paper', maxWidth: 240 }}
    >
      {image ? (
        <Box component="img" src={image} alt="" sx={{ width: 26, height: 26, borderRadius: '7px', objectFit: 'cover', flexShrink: 0 }} />
      ) : (
        <Box sx={{ display: 'inline-flex', color: 'text.secondary', flexShrink: 0 }}>
          <DocumentTextLinear size={13} color="currentColor" />
        </Box>
      )}
      <Typography variant="caption" noWrap sx={{ fontSize: '0.75rem', minWidth: 0 }}>
        {label}
      </Typography>
      {onRemove && (
        <ButtonBase aria-label={`Remove ${label}`} onClick={onRemove} sx={{ borderRadius: '6px', p: 0.25, color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
          <CloseLinear size={12} color="currentColor" />
        </ButtonBase>
      )}
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
}) {
  const theme = useTheme()
  const [text, setText] = useState('')
  const [images, setImages] = useState<ImageAttachment[]>([])
  const [files, setFiles] = useState<FileAttachment[]>([])
  const [dragging, setDragging] = useState(false)
  const ingest = async (list: File[]) => {
    if (!attachments || list.length === 0) return
    const built = await buildAttachments(list)
    setImages((p) => [...p, ...built.images])
    setFiles((p) => [...p, ...built.files])
  }
  const [error, setError] = useState<string | null>(null)
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

  const hasAttachments = images.length + files.length > 0
  const send = async () => {
    const t = text.trim()
    if (!t && !hasAttachments) return
    setError(null)
    const sent = { images, files }
    setText('')
    setImages([])
    setFiles([])
    try {
      await onSend(t, hasAttachments ? sent : undefined)
    } catch (e) {
      setText(t)
      setImages(sent.images)
      setFiles(sent.files)
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
          {running.map(({ agent, run }) => (
            <LiveRunCard key={agent.id} agent={agent} steps={run!.steps} />
          ))}
          {approvals.map((a) => (
            <ApprovalCard key={a.requestId} a={a} team={team} />
          ))}
        </Box>
      </Box>
      <Box sx={{ px: 2, pb: 1.5, pt: 0.5 }}>
        <Box sx={{ maxWidth: 820, mx: 'auto', position: 'relative' }}>
          {options.length > 0 && (
            <MentionShell title="Mention a teammate" width={360}>
              <Box sx={{ px: 0.5, pb: 0.5 }}>
                {options.map((o, i) => (
                  <MentionRow
                    key={o.id}
                    active={i === pick}
                    onSelect={() => complete(o.id)}
                    onHover={() => setPick(i)}
                    tint={o.agent ? colorOf(o.agent) : theme.palette.primary.main}
                    icon={o.agent ? <Avatar name={o.label} color={colorOf(o.agent)} size={20} /> : <Box component="span" sx={{ fontSize: '0.75rem', fontWeight: 700 }}>@</Box>}
                    title={o.label}
                    subtitle={o.agent ? (o.agent.role || `@${o.id}`) : 'Everyone on the team'}
                    tag={<Box component="span" sx={{ fontSize: '0.625rem', color: 'text.disabled', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>@{o.id}</Box>}
                  />
                ))}
              </Box>
            </MentionShell>
          )}
          <Stack
            onDragOver={(e) => {
              if (!attachments) return
              e.preventDefault()
              setDragging(true)
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(e) => {
              if (!attachments) return
              e.preventDefault()
              setDragging(false)
              void ingest(filesFromDataTransfer(e.dataTransfer))
            }}
            sx={{
              gap: 0.75,
              px: 1.5,
              pt: 1,
              pb: 0.75,
              borderRadius: '16px',
              border: 1,
              borderColor: dragging ? 'primary.main' : 'divider',
              bgcolor: 'background.card',
              transition: 'border-color 120ms',
            }}
          >
            {hasAttachments && (
              <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
                {images.map((img, i) => (
                  <Chip
                    key={`img${i}`}
                    label={`Image ${i + 1}`}
                    kind="image"
                    image={`data:${img.media_type};base64,${img.base64}`}
                    onRemove={() => setImages((p) => p.filter((_, j) => j !== i))}
                  />
                ))}
                {files.map((f, i) => (
                  <Chip key={`file${i}`} label={f.name} kind="file" onRemove={() => setFiles((p) => p.filter((_, j) => j !== i))} />
                ))}
              </Stack>
            )}
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
              onPaste={(e) => {
                const pasted = filesFromDataTransfer(e.clipboardData)
                if (attachments && pasted.length > 0) {
                  e.preventDefault()
                  void ingest(pasted)
                }
              }}
              sx={{ width: '100%', fontSize: '0.875rem', py: 0.25 }}
            />
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
              {attachments && (
                <AttachFileButton onAddImages={(i) => setImages((p) => [...p, ...i])} onAddFiles={(f) => setFiles((p) => [...p, ...f])} />
              )}
              {toolbar}
              <Box sx={{ flex: 1 }} />
              <ButtonBase
                aria-label="Send"
                onClick={() => void send()}
                disabled={!text.trim() && !hasAttachments}
                sx={{
                  width: 30,
                  height: 30,
                  borderRadius: '50%',
                  bgcolor: text.trim() || hasAttachments ? 'text.primary' : 'action.disabledBackground',
                  color: 'background.paper',
                  flexShrink: 0,
                }}
              >
                <SendLinear size={15} color="currentColor" />
              </ButtonBase>
            </Stack>
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
