import { useEffect, useRef, useState } from 'react'
import { Box, ButtonBase, Orb, Stack, Typography, alpha, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import { contentToText, type AgentMessage } from '../../ai/lib/types'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey } from '../store/botsStore'
import { LiveAvatar } from './bits'
import { LEAD_CHANNEL } from './TreeTeam'
import { useMasterStore } from '../../sessions/store/masterStore'

const NONE: TeamAgent[] = []

/** What a step of a transcript says, as one line for the preview. */
function lineOf(m: AgentMessage): string | null {
  if (m.type === 'toolResult') return `${m.toolName}${m.isError ? ' (failed)' : ''}`
  if (m.type === 'assistant') {
    const text = contentToText(m.content).trim()
    if (text) return text
    const call = m.content.find((c) => c.type === 'toolCall')
    return call && 'name' in call ? `→ ${call.name}` : null
  }
  if (m.type === 'user') return `You: ${contentToText(m.content).trim()}`
  return null
}

/** A teammate's live transcript, small: the last few steps, refreshed while
 * it is showing. */
function Preview({ root, agent }: { root: string; agent: TeamAgent }) {
  const [msgs, setMsgs] = useState<AgentMessage[] | null>(null)
  useEffect(() => {
    let live = true
    const load = () =>
      void call<AgentMessage[]>('cmd_bots_thread', { root, botId: agent.id })
        .then((m) => live && setMsgs(m ?? []))
        .catch(() => live && setMsgs([]))
    load()
    const t = window.setInterval(load, 2000)
    return () => {
      live = false
      window.clearInterval(t)
    }
  }, [root, agent.id])
  const lines = (msgs ?? []).map(lineOf).filter((l): l is string => !!l).slice(-8)
  return (
    <Stack sx={{ gap: '4px' }}>
      <Typography sx={{ fontSize: '12px', fontWeight: 600 }}>{agent.name}</Typography>
      {msgs === null ? (
        <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>Loading…</Typography>
      ) : lines.length === 0 ? (
        <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>Nothing yet.</Typography>
      ) : (
        lines.map((l, i) => (
          <Typography key={i} sx={{ fontSize: '12px', lineHeight: '16px', color: i === lines.length - 1 ? 'text.primary' : 'text.secondary', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
            {l}
          </Typography>
        ))
      )}
      <Typography sx={{ fontSize: '11px', color: 'text.disabled', pt: '4px' }}>Click to open</Typography>
    </Stack>
  )
}

/**
 * A team project's people, down the chat's left edge: the lead, then each
 * teammate with its face and name. Hover one for its live transcript in a
 * small card; click to show it in the middle. The lead is where you talk.
 */
export function TeamStrip({ root, leadName, leadColor }: { root: string; leadName: string | null; leadColor: string }) {
  const theme = useTheme()
  const team = useTeamStore((s) => s.byRoot[root] ?? NONE).filter((a) => a.status === 'active')
  const status = useBotsStore((s) => s.status)
  const channel = useBotsStore((s) => s.channel[root] ?? LEAD_CHANNEL)
  const { setChannel, loadStatus } = useBotsStore.getState()
  const [hover, setHover] = useState<{ agent: TeamAgent; top: number } | null>(null)
  const hideTimer = useRef<number | undefined>(undefined)
  const stripRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    void useTeamStore.getState().load(root)
    void useMasterStore.getState().load(root)
    void loadStatus(root)
  }, [root, loadStatus])

  const onLead = channel === LEAD_CHANNEL || channel === 'room' || channel.startsWith('group:')
  const row = (active: boolean) =>
    ({
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-start',
      gap: '8px',
      width: '100%',
      height: 36,
      px: '8px',
      borderRadius: '8px',
      textAlign: 'left',
      bgcolor: active ? alpha(theme.palette.primary.main, 0.12) : 'transparent',
      '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
    }) as const

  const show = (agent: TeamAgent, el: HTMLElement) => {
    window.clearTimeout(hideTimer.current)
    const base = stripRef.current?.getBoundingClientRect().top ?? 0
    setHover({ agent, top: el.getBoundingClientRect().top - base })
  }
  const hide = () => {
    window.clearTimeout(hideTimer.current)
    hideTimer.current = window.setTimeout(() => setHover(null), 150)
  }

  return (
    <Box ref={stripRef} sx={{ position: 'absolute', left: 12, top: 12, width: 180, zIndex: 3 }}>
      <Stack sx={{ gap: '2px' }}>
        {leadName && (
          <ButtonBase onClick={() => setChannel(root, LEAD_CHANNEL)} aria-current={onLead ? 'true' : undefined} sx={row(onLead)}>
            <Box sx={{ width: 22, height: 22, borderRadius: '50%', flexShrink: 0, display: 'grid', placeItems: 'center', bgcolor: alpha(leadColor, 0.2), color: leadColor, fontSize: '11px', fontWeight: 700 }}>
              {leadName.trim()[0]?.toUpperCase() ?? 'L'}
            </Box>
            <Box component="span" sx={{ flex: 1, minWidth: 0, fontSize: '13px', fontWeight: onLead ? 600 : 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {leadName}
            </Box>
          </ButtonBase>
        )}
        {team.map((a) => {
          const working = status[botKey(root, a.id)]?.state === 'working'
          const active = channel === a.id
          return (
            <ButtonBase
              key={a.id}
              onClick={() => setChannel(root, a.id)}
              onMouseEnter={(e) => show(a, e.currentTarget as HTMLElement)}
              onMouseLeave={hide}
              aria-current={active ? 'true' : undefined}
              sx={row(active)}
            >
              <LiveAvatar root={root} agent={a} size={22} animate />
              <Box component="span" sx={{ flex: 1, minWidth: 0, fontSize: '13px', fontWeight: active ? 600 : 500, color: active ? 'text.primary' : 'text.secondary', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {a.name}
              </Box>
              {working && <Orb state="working" px={12} label={`${a.name} is working`} />}
            </ButtonBase>
          )
        })}
      </Stack>
      {hover && (
        <Box
          onMouseEnter={() => window.clearTimeout(hideTimer.current)}
          onMouseLeave={hide}
          onClick={() => {
            setChannel(root, hover.agent.id)
            setHover(null)
          }}
          sx={{ position: 'absolute', left: 188, top: hover.top, width: 300, p: '12px', borderRadius: '12px', bgcolor: 'background.paper', border: 1, borderColor: 'divider', boxShadow: theme.elevation.lg, cursor: 'pointer' }}
        >
          <Preview root={root} agent={hover.agent} />
        </Box>
      )}
    </Box>
  )
}
