import { lazy, Suspense, useEffect, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography } from '../../ui'
import { AddLinear, SearchNormalLinear } from '../../icons'
import { useTeamStore, type TeamAgent } from '../team/store/teamStore'
import { useGeneralAgents } from '../team/store/generalAgentsStore'
import { useBotsStore, botKey } from '../team/store/botsStore'
import { Avatar, faceOf } from '../team/components/bits'
import { useBotFace } from '../team/components/AgentFace'
import { NewAgentDialog } from '../team/components/NewAgentDialog'

const TeamRoom = lazy(() => import('../team/components/TeamRoom').then((m) => ({ default: m.TeamRoom })))
const NO_AGENTS: TeamAgent[] = []

function RowFace({ root, agent }: { root: string | null; agent: TeamAgent }) {
  const face = useBotFace(root, agent.id)
  return <Avatar {...faceOf(agent)} size={32} state={face} animate />
}

/**
 * The 24/7 agents on their own: every agent down the left, the one picked
 * on the right with its conversation. Not the General chat's sidebar, which
 * is for threads.
 */
export function AgentsView() {
  const root = useGeneralAgents((s) => s.root)
  const ensure = useGeneralAgents((s) => s.ensure)
  const agents = useTeamStore((s) => (root ? (s.byRoot[root] ?? NO_AGENTS) : NO_AGENTS)).filter((a) => a.status === 'active')
  const channel = useBotsStore((s) => (root ? s.channel[root] : undefined))
  const statuses = useBotsStore((s) => s.status)
  const setChannel = useBotsStore((s) => s.setChannel)
  const loadStatus = useBotsStore((s) => s.loadStatus)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    void ensure().then((r) => r && void loadStatus(r))
  }, [ensure, loadStatus])

  // Land on an agent: the one picked last, or the first.
  const picked = agents.find((a) => a.id === channel) ?? null
  useEffect(() => {
    if (root && !picked && agents[0]) setChannel(root, agents[0].id)
  }, [root, picked, agents, setChannel])

  const q = query.trim().toLowerCase()
  const shown = q ? agents.filter((a) => `${a.name} ${a.role}`.toLowerCase().includes(q)) : agents

  return (
    <Stack direction="row" sx={{ flex: 1, minWidth: 0, height: '100%', bgcolor: 'background.chrome', pb: 1, pr: 1, gap: 1 }}>
      <Stack sx={{ width: 280, flexShrink: 0, borderRadius: '12px', bgcolor: 'background.paper', border: 1, borderColor: 'divider', overflow: 'hidden', ml: 1 }}>
        <Stack sx={{ p: '12px', gap: '8px' }}>
          <Stack direction="row" sx={{ alignItems: 'center', height: 28, px: '4px' }}>
            <Typography sx={{ flex: 1, fontSize: '13px', fontWeight: 600, color: 'text.secondary' }}>24/7 agents</Typography>
            <ButtonBase onClick={() => setCreating(true)} disabled={!root} aria-label="New agent" title="New agent" sx={{ width: 24, height: 24, borderRadius: '6px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
              <AddLinear size={14} color="currentColor" />
            </ButtonBase>
          </Stack>
          <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 32, px: '8px', borderRadius: '8px', border: 1, borderColor: 'divider', color: 'text.disabled' }}>
            <SearchNormalLinear size={13} color="currentColor" />
            <InputBase value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search" sx={{ flex: 1, fontSize: '13px', color: 'text.primary' }} />
          </Stack>
        </Stack>
        <Stack sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '8px', pb: '8px', gap: '2px' }}>
          {shown.length === 0 && (
            <Typography sx={{ fontSize: '13px', color: 'text.secondary', p: '8px' }}>{q ? 'No agents match.' : 'No 24/7 agents yet.'}</Typography>
          )}
          {shown.map((a) => {
            const st = root ? statuses[botKey(root, a.id)] : undefined
            const line = st?.state === 'working' ? (st.activity ?? 'Working') : st?.state === 'paused' ? 'Paused' : a.role
            const on = a.id === picked?.id
            return (
              <ButtonBase
                key={a.id}
                onClick={() => root && setChannel(root, a.id)}
                aria-current={on ? 'true' : undefined}
                sx={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-start', gap: '12px', p: '8px', borderRadius: '8px', textAlign: 'left', bgcolor: on ? 'action.selected' : 'transparent', '&:hover': { bgcolor: on ? 'action.selected' : 'action.hover' } }}
              >
                <RowFace root={root} agent={a} />
                <Stack sx={{ flex: 1, minWidth: 0 }}>
                  <Box component="span" sx={{ fontSize: '14px', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {a.name}
                  </Box>
                  <Box component="span" sx={{ fontSize: '12px', color: 'text.secondary', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {line}
                  </Box>
                </Stack>
              </ButtonBase>
            )
          })}
        </Stack>
      </Stack>

      <Box sx={{ flex: 1, minWidth: 0, borderRadius: '12px', bgcolor: 'background.paper', border: 1, borderColor: 'divider', overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
        {root && picked ? (
          <Suspense fallback={null}>
            <TeamRoom key={root} root={root} />
          </Suspense>
        ) : (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
            <Typography sx={{ fontSize: '13px', color: 'text.secondary' }}>Pick an agent, or make one.</Typography>
          </Stack>
        )}
      </Box>
      {root && <NewAgentDialog root={root} open={creating} count={agents.length} onClose={() => setCreating(false)} onCreated={(a) => setChannel(root, a.id)} />}
    </Stack>
  )
}
