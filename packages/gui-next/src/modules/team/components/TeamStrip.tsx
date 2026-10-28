import { useEffect, useRef, useState } from 'react'
import { Box, ButtonBase, Orb, Stack, Typography, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import type { SxObject } from '../../../ui/sx'
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

/** Loads the team, the lead and their status for a team project's chat.
 * Draws nothing. */
export function TeamStrip({ root }: { root: string; projectName?: string }) {
  useEffect(() => {
    void useTeamStore.getState().load(root)
    void useMasterStore.getState().load(root)
    void useBotsStore.getState().loadStatus(root)
  }, [root])
  return null
}

/**
 * The hired specialists, as the right column's rows: face, name, and a glow
 * while working. Hover one for its live transcript in a card beside the
 * column; click to show its work in the middle (read only).
 */
export function Specialists({ root, rowSx, labelSx }: { root: string; rowSx: SxObject; labelSx: SxObject }) {
  const theme = useTheme()
  const team = useTeamStore((s) => s.byRoot[root] ?? NONE).filter((a) => a.status === 'active')
  const status = useBotsStore((s) => s.status)
  const channel = useBotsStore((s) => s.channel[root] ?? LEAD_CHANNEL)
  const setChannel = useBotsStore((s) => s.setChannel)
  const [hover, setHover] = useState<{ agent: TeamAgent; top: number } | null>(null)
  const hideTimer = useRef<number | undefined>(undefined)
  const listRef = useRef<HTMLDivElement | null>(null)

  const show = (agent: TeamAgent, el: HTMLElement) => {
    window.clearTimeout(hideTimer.current)
    const base = listRef.current?.getBoundingClientRect().top ?? 0
    setHover({ agent, top: el.getBoundingClientRect().top - base })
  }
  const hide = () => {
    window.clearTimeout(hideTimer.current)
    hideTimer.current = window.setTimeout(() => setHover(null), 150)
  }

  return (
    <Box ref={listRef} sx={{ position: 'relative' }}>
      <Typography sx={labelSx}>Specialists</Typography>
      {team.length === 0 ? (
        <Typography sx={{ fontSize: '12px', color: 'text.disabled', px: '8px' }}>None hired yet.</Typography>
      ) : (
        team.map((a) => {
          const working = status[botKey(root, a.id)]?.state === 'working'
          const active = channel === a.id
          return (
            <ButtonBase
              key={a.id}
              onClick={() => setChannel(root, active ? LEAD_CHANNEL : a.id)}
              onMouseEnter={(e) => show(a, e.currentTarget as HTMLElement)}
              onMouseLeave={hide}
              aria-current={active ? 'true' : undefined}
              sx={{ ...rowSx, gap: '8px', bgcolor: active ? 'action.selected' : 'transparent', color: active ? 'text.primary' : 'text.secondary' }}
            >
              <LiveAvatar root={root} agent={a} size={18} animate />
              <Box component="span" sx={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left' }}>
                {a.name}
              </Box>
              {working && <Orb state="working" px={12} label={`${a.name} is working`} />}
            </ButtonBase>
          )
        })
      )}
      {hover && (
        <Box
          onMouseEnter={() => window.clearTimeout(hideTimer.current)}
          onMouseLeave={hide}
          onClick={() => {
            setChannel(root, hover.agent.id)
            setHover(null)
          }}
          sx={{ position: 'absolute', right: 'calc(100% + 8px)', top: hover.top, width: 300, p: '12px', borderRadius: '12px', bgcolor: 'background.paper', border: 1, borderColor: 'divider', boxShadow: theme.elevation.lg, cursor: 'pointer' }}
        >
          <Preview root={root} agent={hover.agent} />
        </Box>
      )}
    </Box>
  )
}
