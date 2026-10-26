import { useEffect, type ReactNode } from 'react'
import { Box, ButtonBase, IconButton, Orb, Stack, Tooltip, Typography, alpha } from '../../../ui'
import { AddLinear, DocumentTextLinear, StarLinear } from '../../../icons'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey } from '../store/botsStore'
import { LiveAvatar } from './bits'
import { ChannelRow, SECTION_LABEL_SX, preview } from './TeamConversations'

/** The channel a tree's lead conversation shows in, when its lead is the
 * project's master (a project made as a company). */
export const LEAD_CHANNEL = '__lead'

/**
 * A tree project in the sidebar: its circles, and the team as an org chart
 * (who leads, who reports to whom). Picking a circle or a bot opens it in the
 * chat panel. With no lead, the user leads.
 */
export function TreeTeam({
  root,
  team,
  master,
  pendingHires,
  onHire,
  onCharter,
}: {
  root: string
  team: TeamAgent[]
  /** The project conversation's lead, when the tree was made as a company. */
  master: { name: string; color: string } | null
  pendingHires: number
  onHire: () => void
  onCharter: () => void
}) {
  const room = useBotsStore((s) => s.room[root])
  const dm = useBotsStore((s) => s.dm)
  const status = useBotsStore((s) => s.status)
  const unread = useBotsStore((s) => s.unread)
  const channel = useBotsStore((s) => s.channel[root] ?? 'room')
  const approvals = useBotsStore((s) => s.approvals)
  const save = useTeamStore((s) => s.save)
  const { setChannel, loadRoom, loadDm, loadStatus } = useBotsStore.getState()

  useEffect(() => {
    void loadRoom(root)
    void loadStatus(root)
    for (const a of team) void loadDm(root, a.id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, team.map((a) => a.id).join(',')])

  const working = team.filter((a) => status[botKey(root, a.id)]?.state === 'working')
  const roomLast = room?.[room.length - 1]
  const known = new Set(team.map((a) => a.id))
  // Who a bot reports to in the chart: another bot, or the top (the master
  // when there is one, else the user).
  const boss = (a: TeamAgent) => (a.reportsTo && known.has(a.reportsTo) ? a.reportsTo : null)
  const under = (id: string | null) =>
    team.filter((a) => boss(a) === id).sort((x, y) => Number(!!y.lead) - Number(!!x.lead) || x.name.localeCompare(y.name))
  const noLead = !master && !team.some((a) => a.lead)

  const botRow = (a: TeamAgent, depth: number): ReactNode => {
    const st = status[botKey(root, a.id)]
    const msgs = dm[botKey(root, a.id)]
    const waiting = approvals.some((p) => p.root === root && p.botId === a.id)
    const state = st?.state ?? 'idle'
    const what = a.lead ? (a.role ? `Lead · ${a.role}` : 'Lead') : a.role || 'Specialist'
    return (
      <Box key={a.id}>
        <Box sx={{ position: 'relative', pl: depth * 1.75, '&:hover .tree-promote': { opacity: 1 } }}>
          <ChannelRow
            active={channel === a.id}
            onClick={() => setChannel(root, a.id)}
            icon={
              <Box sx={{ position: 'relative', flexShrink: 0 }}>
                <LiveAvatar root={root} agent={a} size={26} animate />
                {state === 'working' && !waiting && (
                  <Box sx={{ position: 'absolute', right: -4, bottom: -4, display: 'flex', borderRadius: '50%', bgcolor: 'background.paper', p: '1px' }}>
                    <Orb state="working" px={13} />
                  </Box>
                )}
                {waiting && (
                  <Box sx={{ position: 'absolute', right: -1, bottom: -1, width: 9, height: 9, borderRadius: '50%', border: 2, borderColor: 'background.paper', bgcolor: 'warning.main' }} />
                )}
              </Box>
            }
            title={a.name}
            subtitle={
              waiting ? 'Waiting for your approval' : state === 'working' ? (st?.activity ?? 'Working…') : (preview(msgs?.[msgs.length - 1], team) ?? what)
            }
            subtitleTone={waiting ? 'warning.main' : state === 'working' ? 'success.main' : undefined}
            unread={unread[botKey(root, a.id)] ?? 0}
          />
          <Tooltip title={a.lead ? 'Remove as lead' : 'Make lead: it hires into its branch and hands out its work'}>
            <IconButton
              size="small"
              className="tree-promote"
              aria-label={a.lead ? `Remove ${a.name} as lead` : `Make ${a.name} a lead`}
              onClick={() => void save(root, { ...a, lead: !a.lead }, a.id)}
              sx={{
                position: 'absolute',
                right: 4,
                top: '50%',
                transform: 'translateY(-50%)',
                opacity: a.lead ? 1 : 0,
                color: a.lead ? 'warning.main' : 'text.secondary',
                transition: 'opacity 0.12s',
              }}
            >
              <StarLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Box>
        {under(a.id).map((c) => botRow(c, depth + 1))}
      </Box>
    )
  }

  return (
    <Stack sx={{ px: 1.5, gap: 1.5 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, px: 0.5, pb: 0.5 }}>
          Circles
        </Typography>
        <ChannelRow
          active={channel === 'room'}
          onClick={() => setChannel(root, 'room')}
          icon={
            <Box
              sx={{
                width: 26,
                height: 26,
                borderRadius: '8px',
                display: 'grid',
                placeItems: 'center',
                bgcolor: 'action.hover',
                color: 'text.secondary',
                flexShrink: 0,
                fontWeight: 700,
                fontSize: '0.875rem',
              }}
            >
              #
            </Box>
          }
          title="Everyone"
          subtitle={working.length ? `${working.map((a) => a.name).join(', ')} working…` : (preview(roomLast, team) ?? 'Say what you need')}
          subtitleTone={working.length ? 'success.main' : undefined}
          unread={unread[botKey(root, 'room')] ?? 0}
        />
      </Stack>

      <Stack sx={{ gap: 0.25 }}>
        <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
            Team{team.length ? ` · ${team.length}` : ''}
          </Typography>
          {master && (
            <Tooltip title="Charter">
              <IconButton size="small" aria-label="Open the charter" onClick={onCharter}>
                <DocumentTextLinear size={13} color="currentColor" />
              </IconButton>
            </Tooltip>
          )}
          <Tooltip title="Hire a specialist">
            <IconButton size="small" aria-label="Hire a specialist" onClick={onHire}>
              <AddLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
        {pendingHires > 0 && (
          <ButtonBase
            onClick={onHire}
            sx={{ justifyContent: 'flex-start', px: 1, py: 0.625, borderRadius: '8px', bgcolor: 'action.selected', color: 'primary.main' }}
          >
            <Typography variant="caption" sx={{ fontWeight: 600, fontSize: '0.75rem' }}>
              {pendingHires} proposed hire{pendingHires === 1 ? '' : 's'} to approve
            </Typography>
          </ButtonBase>
        )}
        {master && (
          <ChannelRow
            active={channel === LEAD_CHANNEL}
            onClick={() => setChannel(root, LEAD_CHANNEL)}
            icon={
              <Box
                sx={{
                  width: 26,
                  height: 26,
                  borderRadius: '50%',
                  flexShrink: 0,
                  display: 'grid',
                  placeItems: 'center',
                  bgcolor: alpha(master.color, 0.2),
                  color: master.color,
                  fontSize: '0.75rem',
                  fontWeight: 700,
                }}
              >
                {master.name.trim()[0]?.toUpperCase() ?? 'L'}
              </Box>
            }
            title={master.name}
            subtitle="Lead · tell it your goals"
            unread={0}
          />
        )}
        {noLead && team.length > 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, pb: 0.5, lineHeight: 1.45 }}>
            You lead this team. Star a bot to make it lead.
          </Typography>
        )}
        {under(null).map((a) => botRow(a, master ? 1 : 0))}
        {team.length === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, py: 0.5, lineHeight: 1.45 }}>
            No one on the team yet. Hire with + — each bot gets its own role, model, tools and memory.
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
