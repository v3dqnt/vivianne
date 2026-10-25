import { useEffect } from 'react'
import { Box, ButtonBase, IconButton, Orb, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { AddLinear, SettingsLinear } from '../../../icons'
import { openPanel } from '../../workspace/layout'
import { useGeneralAgents } from '../store/generalAgentsStore'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey } from '../store/botsStore'
import { useBotFace } from './AgentFace'
import { Avatar, faceOf } from './bits'

const EMPTY: TeamAgent[] = []

/**
 * Agents of your own in the General tab: each with its own instructions,
 * model, tools, memory and routines, and an email address and phone number
 * if you give it one. Click one to talk to it; the gear opens its page.
 */
export function GeneralAgents({ labelSx }: { labelSx: object }) {
  const theme = useTheme()
  const root = useGeneralAgents((s) => s.root)
  const ensure = useGeneralAgents((s) => s.ensure)
  const agents = useTeamStore((s) => (root ? (s.byRoot[root] ?? EMPTY) : EMPTY)).filter((a) => a.status === 'active')
  const focus = useTeamStore((s) => s.focus)
  const channel = useBotsStore((s) => (root ? s.channel[root] : undefined))
  const statuses = useBotsStore((s) => s.status)
  const setChannel = useBotsStore((s) => s.setChannel)
  const loadStatus = useBotsStore((s) => s.loadStatus)

  useEffect(() => {
    void ensure().then((r) => r && void loadStatus(r))
  }, [ensure, loadStatus])

  const manage = (id: string | null) => {
    if (!root) return
    focus(root, id)
    openPanel('agents')
  }

  return (
    <Box sx={{ px: 1.5, pb: 1.25, flexShrink: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
        <Typography variant="caption" color="text.secondary" sx={{ ...labelSx, flex: 1 }}>
          Agents{agents.length ? ` · ${agents.length}` : ''}
        </Typography>
        <Tooltip title="Create an agent">
          <IconButton size="small" aria-label="Create an agent" onClick={() => manage(null)}>
            <AddLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      {agents.length === 0 ? (
        <ButtonBase
          onClick={() => manage(null)}
          sx={{ display: 'block', width: '100%', textAlign: 'left', px: 1, py: 0.75, borderRadius: '8px', border: 1, borderStyle: 'dashed', borderColor: 'divider', '&:hover': { bgcolor: 'action.hover' } }}
        >
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
            Make an agent of your own: its job, tools and model, and its own email and phone number. It works on its own, on a schedule
            or when you message it.
          </Typography>
        </ButtonBase>
      ) : (
        <Stack sx={{ gap: 0.25 }}>
          {agents.map((a) => {
            const st = root ? statuses[botKey(root, a.id)] : undefined
            const working = st?.state === 'working'
            const active = channel === a.id
            return (
              <Stack key={a.id} direction="row" sx={{ alignItems: 'center', '&:hover .agent-gear': { opacity: 1 } }}>
                <ButtonBase
                  onClick={() => root && setChannel(root, a.id)}
                  sx={{
                    flex: 1,
                    minWidth: 0,
                    justifyContent: 'flex-start',
                    gap: 1,
                    px: 1,
                    py: 0.5,
                    borderRadius: '8px',
                    bgcolor: active ? alpha(theme.palette.primary.main, 0.1) : 'transparent',
                    '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
                  }}
                >
                  <RowFace root={root} agent={a} />
                  <Stack sx={{ minWidth: 0, flex: 1, textAlign: 'left' }}>
                    <Typography variant="body2" noWrap sx={{ fontSize: '0.8125rem', fontWeight: active ? 600 : 500 }}>
                      {a.name}
                    </Typography>
                    <Typography variant="caption" color={working ? 'primary.main' : 'text.secondary'} noWrap sx={{ fontSize: '0.6875rem' }}>
                      {working ? (st?.activity ?? 'Working') : a.role || 'Idle'}
                    </Typography>
                  </Stack>
                  {working && <Orb state="working" px={16} />}
                </ButtonBase>
                <Tooltip title="Its page: instructions, contact, routines, rules">
                  <IconButton size="small" className="agent-gear" aria-label={`Open ${a.name}'s page`} onClick={() => manage(a.id)} sx={{ opacity: 0, transition: 'opacity 0.12s' }}>
                    <SettingsLinear size={13} color="currentColor" />
                  </IconButton>
                </Tooltip>
              </Stack>
            )
          })}
        </Stack>
      )}
    </Box>
  )
}

/** A row's face, live: its own component so each row subscribes to its own
 * agent's state rather than the list re-rendering for all of them. */
function RowFace({ root, agent }: { root: string | null; agent: TeamAgent }) {
  const face = useBotFace(root, agent.id)
  return <Avatar {...faceOf(agent)} size={24} state={face} animate />
}
