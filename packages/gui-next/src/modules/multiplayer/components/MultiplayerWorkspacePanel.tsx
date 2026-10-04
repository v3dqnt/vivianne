import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Box, Button, ButtonBase, CircularProgress, InputBase, Stack, Tooltip, Typography, alpha, keyframes, useTheme } from '../../../ui'
import { CodeLinear, CopyLinear, SendLinear, TickCircleLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { useMultiplayerStore, type ChatEntry } from '../store/multiplayerStore'
import { useProjectsStore, type Project } from '../../sessions/store/projectsStore'
import { Markdown } from '../../ai/components/Markdown'
import { FileLink } from '../../ai/components/FileLink'
import { inviteMessage } from '../lib/invite'
import { UsageBlock, money, sum, today, type ActivityEntry, type CollabData } from './CollabView'

const pulse = keyframes`
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
`

type Presence = 'live' | 'connecting' | 'offline' | 'failed'

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase())
    .join('')
}

function Face({ name, color, size = 28 }: { name: string; color: string; size?: number }) {
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: '50%',
        flexShrink: 0,
        display: 'grid',
        placeItems: 'center',
        fontSize: size * 0.38,
        fontWeight: 700,
        bgcolor: alpha(color, 0.16),
        color,
      }}
    >
      {initials(name) || '?'}
    </Box>
  )
}

function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86_400) return `${Math.floor(s / 3600)}h ago`
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

function dayLabel(ms: number): string {
  const d = new Date(ms)
  const now = new Date()
  const y = new Date(now)
  y.setDate(now.getDate() - 1)
  if (d.toDateString() === now.toDateString()) return 'Today'
  if (d.toDateString() === y.toDateString()) return 'Yesterday'
  return d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })
}

/** Live collaboration data for a project, refreshed when the backend says so. */
function useCollab(projectId: string): [CollabData | null, () => void] {
  const [data, setData] = useState<CollabData | null>(null)
  const load = () => {
    if (projectId) void call<CollabData>('cmd_multiplayer_collab', { projectId }).then((d) => d && setData(d))
  }
  useEffect(() => {
    setData(null)
    load()
    let off: (() => void) | undefined
    void on<string>('multiplayer_collab', (id) => {
      if (id === projectId) load()
    }).then((u) => (off = u ?? undefined))
    return () => off?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])
  return [data, load]
}

// ── Header ────────────────────────────────────────────────────────────────

function Header({ project, presence, data }: { project: Project; presence: Presence; data: CollabData | null }) {
  const theme = useTheme()
  const mp = project.multiplayer!
  const partnerName = mp.peer_display_name || 'Your partner'
  const status = useMultiplayerStore((s) => s.status)
  const connecting = useMultiplayerStore((s) => s.connecting)
  const connect = useMultiplayerStore((s) => s.connect)
  const disconnect = useMultiplayerStore((s) => s.disconnect)
  const activeAt = useMultiplayerStore((s) => s.partnerActiveAt)
  const [, tick] = useState(0)
  useEffect(() => {
    const t = window.setInterval(() => tick((n) => n + 1), 10_000)
    return () => window.clearInterval(t)
  }, [])
  const working = presence === 'live' && Date.now() - activeAt < 30_000
  const dot = presence === 'live' ? 'success.main' : presence === 'connecting' ? 'warning.main' : presence === 'failed' ? 'error.main' : 'text.disabled'
  const label =
    presence === 'live'
      ? working
        ? `${partnerName}'s agent is working…`
        : 'Online'
      : presence === 'connecting'
        ? 'Connecting…'
        : presence === 'failed'
          ? status.state === 'failed'
            ? status.reason
            : "Couldn't connect"
          : 'Offline'
  const mineToday = data ? sum(data.myUsage, today()) : null
  const theirsToday = data ? sum(data.partnerUsage, today()) : null

  return (
    <Stack sx={{ gap: 1, px: 1.75, pt: 1.5, pb: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.25 }}>
        <Box sx={{ position: 'relative' }}>
          <Face name={partnerName} color={theme.palette.success.main} size={34} />
          <Box
            sx={{
              position: 'absolute',
              right: -1,
              bottom: -1,
              width: 11,
              height: 11,
              borderRadius: '50%',
              bgcolor: dot,
              border: 2,
              borderColor: 'background.paper',
              animation: working || presence === 'connecting' ? `${pulse} 1.2s ease-in-out infinite` : undefined,
            }}
          />
        </Box>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 700 }} noWrap>
            {partnerName}
          </Typography>
          <Typography variant="caption" color={presence === 'failed' ? 'error.main' : working ? 'success.main' : 'text.secondary'} noWrap>
            {label}
          </Typography>
        </Stack>
        {presence === 'live' ? (
          <Button size="small" color="inherit" onClick={() => void disconnect()} sx={{ color: 'text.secondary', borderRadius: '8px' }}>
            Disconnect
          </Button>
        ) : (
          <Button
            size="small"
            variant="contained"
            disableElevation
            disabled={connecting || presence === 'connecting'}
            onClick={() => void connect(mp.peer_id).catch(() => {})}
            startIcon={connecting ? <CircularProgress size={12} /> : undefined}
            sx={{ borderRadius: '8px' }}
          >
            {presence === 'failed' ? 'Try again' : 'Connect'}
          </Button>
        )}
      </Stack>
      <Stack direction="row" sx={{ gap: 1.5, flexWrap: 'wrap' }}>
        {mineToday && theirsToday && (
          <Typography variant="caption" color="text.secondary">
            Today · you {money(mineToday)} · {partnerName.split(' ')[0]} {money(theirsToday)}
          </Typography>
        )}
        <Typography variant="caption" color="text.disabled">
          {data?.pendingMerge ? 'Their latest changes merge before your next run' : (data?.lastSync ?? 'Each run is committed and shared')}
        </Typography>
      </Stack>
    </Stack>
  )
}

function SyncBanner({ data, projectId, onChange }: { data: CollabData; projectId: string; onChange: () => void }) {
  const theme = useTheme()
  if (data.notes.length === 0) return null
  return (
    <Stack sx={{ mx: 1.75, mt: 1.25, p: 1.25, gap: 0.75, borderRadius: '10px', border: 1, borderColor: alpha(theme.palette.warning.main, 0.4), bgcolor: alpha(theme.palette.warning.main, 0.06) }}>
      <Typography variant="caption" sx={{ fontWeight: 700, color: 'warning.main' }}>
        Code sync needs a look
      </Typography>
      {data.notes.map((n) => (
        <Typography key={n.at} variant="caption" sx={{ lineHeight: 1.45 }}>
          {n.text}
        </Typography>
      ))}
      <ButtonBase
        onClick={() => void call('cmd_multiplayer_dismiss_notes', { projectId }).then(onChange)}
        sx={{ alignSelf: 'flex-start', fontSize: '0.6875rem', fontWeight: 600, color: 'text.secondary', px: 0.5, borderRadius: '6px' }}
      >
        Dismiss
      </ButtonBase>
    </Stack>
  )
}

// ── Chat ──────────────────────────────────────────────────────────────────

function Chat({ partnerName, live }: { partnerName: string; live: boolean }) {
  const theme = useTheme()
  const chatLog = useMultiplayerStore((s) => s.chatLog)
  const sendChat = useMultiplayerStore((s) => s.sendChat)
  const setChatOpen = useMultiplayerStore((s) => s.setChatOpen)
  const myName = useMultiplayerStore((s) => s.displayName) || 'You'
  const [text, setText] = useState('')
  const [snippet, setSnippet] = useState(false)
  const scroller = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    setChatOpen(true)
    return () => setChatOpen(false)
  }, [setChatOpen])
  useLayoutEffect(() => {
    const el = scroller.current
    if (el) el.scrollTop = el.scrollHeight
  }, [chatLog.length])

  const send = () => {
    const t = text.trim()
    if (!t || !live) return
    void sendChat(snippet ? `\`\`\`\n${t}\n\`\`\`` : t)
    setText('')
    setSnippet(false)
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Box ref={scroller} sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.75, py: 1 }}>
        {chatLog.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', textAlign: 'center', py: 4, lineHeight: 1.5 }}>
            {live ? `Say hi, share a snippet, or split up the work with ${partnerName}.` : `Messages appear here once you're connected.`}
          </Typography>
        ) : (
          chatLog.map((m: ChatEntry, i) => {
            const prev = chatLog[i - 1]
            const newDay = !prev || new Date(prev.timestamp).toDateString() !== new Date(m.timestamp).toDateString()
            const grouped = !newDay && prev && prev.fromPeer === m.fromPeer && m.timestamp - prev.timestamp < 5 * 60_000
            const who = m.fromPeer ? partnerName : myName
            const color = m.fromPeer ? theme.palette.success.main : theme.palette.primary.main
            return (
              <Box key={i}>
                {newDay && (
                  <Typography variant="caption" color="text.disabled" sx={{ display: 'block', textAlign: 'center', fontSize: '0.6875rem', my: 1 }}>
                    {dayLabel(m.timestamp)}
                  </Typography>
                )}
                <Stack direction="row" sx={{ gap: 1, mt: grouped ? 0.25 : 1, alignItems: 'flex-start' }}>
                  <Box sx={{ width: 26, flexShrink: 0 }}>{!grouped && <Face name={who} color={color} size={26} />}</Box>
                  <Stack sx={{ minWidth: 0, flex: 1 }}>
                    {!grouped && (
                      <Stack direction="row" sx={{ gap: 0.75, alignItems: 'baseline' }}>
                        <Typography variant="caption" sx={{ fontWeight: 700 }}>
                          {m.fromPeer ? partnerName : 'You'}
                        </Typography>
                        <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
                          {clock(m.timestamp)}
                        </Typography>
                      </Stack>
                    )}
                    <Box sx={{ fontSize: '0.8125rem', '& p': { my: 0.25 }, '& pre': { fontSize: '0.75rem' } }}>
                      <Markdown text={m.text} />
                    </Box>
                  </Stack>
                </Stack>
              </Box>
            )
          })
        )}
      </Box>
      <Box sx={{ px: 1.75, pb: 1.5, pt: 0.5 }}>
        <Stack direction="row" sx={{ alignItems: 'flex-end', gap: 0.75, px: 1, py: 0.75, borderRadius: '14px', border: 1, borderColor: 'divider', bgcolor: 'background.card', opacity: live ? 1 : 0.6 }}>
          <Tooltip title={snippet ? 'Sending as a code snippet' : 'Send as a code snippet'}>
            <ButtonBase
              onClick={() => setSnippet((v) => !v)}
              sx={{ width: 28, height: 28, borderRadius: '8px', color: snippet ? 'primary.main' : 'text.secondary', bgcolor: snippet ? 'action.selected' : 'transparent', flexShrink: 0 }}
            >
              <CodeLinear size={15} color="currentColor" />
            </ButtonBase>
          </Tooltip>
          <InputBase
            multiline
            maxRows={8}
            minRows={snippet ? 3 : 1}
            disabled={!live}
            value={text}
            placeholder={live ? (snippet ? 'Paste a snippet' : `Message ${partnerName}`) : 'Connect to chat'}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !snippet) {
                e.preventDefault()
                send()
              }
            }}
            sx={{ flex: 1, fontSize: '0.8125rem', py: 0.375, ...(snippet ? { fontFamily: '"Geist Mono", ui-monospace, monospace' } : {}) }}
          />
          <ButtonBase
            aria-label="Send"
            onClick={send}
            disabled={!text.trim() || !live}
            sx={{ width: 28, height: 28, borderRadius: '50%', bgcolor: text.trim() && live ? 'text.primary' : 'action.disabledBackground', color: 'background.paper', flexShrink: 0 }}
          >
            <SendLinear size={14} color="currentColor" />
          </ButtonBase>
        </Stack>
      </Box>
    </Stack>
  )
}

// ── Activity ──────────────────────────────────────────────────────────────

function ActivityItem({ e, who, mine }: { e: ActivityEntry; who: string; mine: boolean }) {
  const theme = useTheme()
  const color = mine ? theme.palette.primary.main : theme.palette.success.main
  const failed = e.commands.filter((c) => !c.ok).length
  return (
    <Stack direction="row" sx={{ gap: 1.25, py: 1 }}>
      <Face name={who} color={color} size={26} />
      <Stack sx={{ minWidth: 0, flex: 1, gap: 0.5 }}>
        <Stack direction="row" sx={{ gap: 0.75, alignItems: 'baseline' }}>
          <Typography variant="caption" sx={{ fontWeight: 700 }}>
            {mine ? 'Your agent' : `${who}'s agent`}
          </Typography>
          <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
            {ago(e.at)}
            {e.commit ? ` · ${e.commit.slice(0, 7)}` : ''}
          </Typography>
        </Stack>
        <Typography variant="body2" sx={{ fontSize: '0.8125rem', lineHeight: 1.45 }}>
          {e.request}
        </Typography>
        {e.outcome && (
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
            {e.outcome}
          </Typography>
        )}
        {e.files.length > 0 && (
          <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap', alignItems: 'center' }}>
            {e.files.slice(0, 8).map((f) => (
              <Stack key={f.path} direction="row" sx={{ alignItems: 'center', gap: 0.25 }}>
                <Typography variant="caption" sx={{ fontWeight: 700, color: f.op === 'created' ? 'success.main' : f.op === 'deleted' ? 'error.main' : 'text.secondary', fontSize: '0.6875rem' }}>
                  {f.op === 'created' ? '+' : f.op === 'deleted' ? '−' : '~'}
                </Typography>
                <FileLink path={f.path} />
              </Stack>
            ))}
            {e.files.length > 8 && (
              <Typography variant="caption" color="text.secondary">
                +{e.files.length - 8} more
              </Typography>
            )}
          </Stack>
        )}
        {e.commands.length > 0 && (
          <Typography variant="caption" color={failed ? 'error.main' : 'text.secondary'} sx={{ fontSize: '0.6875rem' }}>
            {failed ? `${failed} of ${e.commands.length} commands failed: ` : `Ran ${e.commands.length} command${e.commands.length === 1 ? '' : 's'}: `}
            <Box component="span" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
              {e.commands
                .slice(0, 3)
                .map((c) => `${c.ok ? '✓' : '✗'} ${c.command.slice(0, 40)}`)
                .join('  ')}
            </Box>
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}

function Activity({ data, partnerName }: { data: CollabData; partnerName: string }) {
  const [who, setWho] = useState<'all' | 'me' | 'partner'>('all')
  const items = useMemo(
    () =>
      [
        ...(who !== 'me' ? data.partner.map((e) => ({ e, mine: false })) : []),
        ...(who !== 'partner' ? data.mine.map((e) => ({ e, mine: true })) : []),
      ].sort((a, b) => b.e.at - a.e.at),
    [data, who],
  )
  const first = partnerName.split(' ')[0]
  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ gap: 0.5, px: 1.75, pt: 1.25 }}>
        {([
          ['all', 'Everyone'],
          ['me', 'You'],
          ['partner', first],
        ] as const).map(([k, label]) => (
          <ButtonBase
            key={k}
            onClick={() => setWho(k)}
            sx={{ px: 1, py: 0.375, borderRadius: '999px', fontSize: '0.6875rem', fontWeight: 600, border: 1, borderColor: who === k ? 'text.primary' : 'divider', color: who === k ? 'text.primary' : 'text.secondary' }}
          >
            {label}
          </ButtonBase>
        ))}
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.75, pb: 1 }}>
        {items.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', py: 4, textAlign: 'center', lineHeight: 1.5 }}>
            Every run either agent finishes shows up here: what was asked, the files it changed and the commands it ran. Each agent is
            told what the other did before its next run.
          </Typography>
        ) : (
          items.map(({ e, mine }) => <ActivityItem key={`${mine}-${e.id}`} e={e} who={mine ? 'You' : partnerName} mine={mine} />)
        )}
      </Box>
    </Stack>
  )
}

// ── Not connected ─────────────────────────────────────────────────────────

function Offline({ project }: { project: Project }) {
  const myId = useMultiplayerStore((s) => s.myId)
  const displayName = useMultiplayerStore((s) => s.displayName)
  const error = useMultiplayerStore((s) => s.error)
  const [copied, setCopied] = useState(false)
  const [isTest, setIsTest] = useState(false)
  const [busy, setBusy] = useState(false)
  const partnerName = project.multiplayer?.peer_display_name || 'your partner'
  useEffect(() => {
    void call<string>('cmd_multiplayer_test_partner_id')
      .then((id) => setIsTest(!!id && id === project.multiplayer?.peer_id))
      .catch(() => setIsTest(false))
  }, [project.multiplayer?.peer_id])
  return (
    <Stack sx={{ gap: 1.5, px: 1.75, py: 2 }}>
      <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
        {partnerName === 'your partner' ? 'Your partner' : partnerName} needs Vivianne open on their side too. Once you're connected you
        can chat, and each of you sees what the other's agent did. Work you do now is shared when you reconnect.
      </Typography>
      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}
      {isTest ? (
        <Button
          size="small"
          variant="outlined"
          disabled={busy}
          onClick={() => {
            setBusy(true)
            void call('cmd_multiplayer_launch_test_partner', { name: project.name, folder: project.path ?? '' }).finally(() => setBusy(false))
          }}
          sx={{ alignSelf: 'flex-start', borderRadius: '8px' }}
        >
          Open the test partner
        </Button>
      ) : (
        myId && (
          <Button
            size="small"
            variant="outlined"
            startIcon={copied ? <TickCircleLinear size={14} color="currentColor" /> : <CopyLinear size={14} color="currentColor" />}
            onClick={() => {
              void navigator.clipboard.writeText(inviteMessage(myId, displayName))
              setCopied(true)
              window.setTimeout(() => setCopied(false), 1800)
            }}
            sx={{ alignSelf: 'flex-start', borderRadius: '8px' }}
          >
            {copied ? 'Copied' : 'Copy your invite again'}
          </Button>
        )
      )}
    </Stack>
  )
}

// ── Panel ─────────────────────────────────────────────────────────────────

type Tab = 'chat' | 'activity' | 'usage'

/**
 * Working together on a multiplayer project: who your partner is and whether
 * they're here, a chat between you, one timeline of what both agents did,
 * and what each of you spent. Sync problems show at the top.
 */
export function MultiplayerWorkspacePanel() {
  const theme = useTheme()
  const project = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId))
  const status = useMultiplayerStore((s) => s.status)
  const unread = useMultiplayerStore((s) => s.unread)
  const init = useMultiplayerStore((s) => s.init)
  const [tab, setTab] = useState<Tab>('chat')

  useEffect(() => {
    void init()
  }, [init])

  const [data, reload] = useCollab(project?.id ?? '')

  if (!project?.multiplayer) {
    return (
      <Stack sx={{ gap: 1, p: 2.5, alignItems: 'flex-start' }}>
        <Typography variant="body2" sx={{ fontWeight: 700 }}>
          Work together
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.55 }}>
          {project ? `"${project.name}" is a solo project. ` : ''}To work with someone, create a shared project: New project → Multiplayer.
          You swap invites, then chat, see what each other's agent did, and keep the code in sync. No second device? You can try it with
          a test partner on this computer.
        </Typography>
      </Stack>
    )
  }

  const presence: Presence =
    status.state === 'connected' && status.peerId === project.multiplayer.peer_id
      ? 'live'
      : status.state === 'connecting'
        ? 'connecting'
        : status.state === 'failed'
          ? 'failed'
          : 'offline'
  const partnerName = project.multiplayer.peer_display_name || 'Partner'
  const tabs: { id: Tab; label: string; badge?: number }[] = [
    { id: 'chat', label: 'Chat', badge: tab === 'chat' ? 0 : unread },
    { id: 'activity', label: 'Activity' },
    { id: 'usage', label: 'Usage' },
  ]

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <Header project={project} presence={presence} data={data} />
      {data && <SyncBanner data={data} projectId={project.id} onChange={reload} />}
      {presence !== 'live' && <Offline project={project} />}
      <Stack direction="row" sx={{ gap: 0.25, px: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        {tabs.map((t) => (
          <ButtonBase
            key={t.id}
            onClick={() => setTab(t.id)}
            sx={{
              gap: 0.5,
              px: 1,
              py: 0.875,
              fontSize: '0.75rem',
              fontWeight: 600,
              color: tab === t.id ? 'text.primary' : 'text.secondary',
              borderBottom: 2,
              borderColor: tab === t.id ? 'text.primary' : 'transparent',
              mb: '-1px',
            }}
          >
            {t.label}
            {!!t.badge && (
              <Box sx={{ minWidth: 16, height: 16, px: 0.5, borderRadius: '8px', bgcolor: 'primary.main', color: 'primary.contrastText', fontSize: '0.625rem', display: 'grid', placeItems: 'center' }}>
                {t.badge}
              </Box>
            )}
          </ButtonBase>
        ))}
      </Stack>
      {tab === 'chat' && <Chat partnerName={partnerName} live={presence === 'live'} />}
      {tab === 'activity' && data && <Activity data={data} partnerName={partnerName} />}
      {tab === 'usage' && data && (
        <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.75, py: 1.5, '& *': { borderColor: alpha(theme.palette.divider, 1) } }}>
          <UsageBlock data={data} partnerName={partnerName} />
        </Box>
      )}
    </Stack>
  )
}
