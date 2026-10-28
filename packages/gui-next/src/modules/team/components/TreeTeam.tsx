import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import {
  Box,
  ButtonBase,
  IconButton,
  Orb,
  Stack,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from '../../../ui'
import { StarLinear } from '../../../icons'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey } from '../store/botsStore'
import { LiveAvatar } from './bits'
import { SECTION_LABEL_SX } from './TeamConversations'

/** The channel a tree's lead conversation shows in, when its lead is the
 * project's master (a project made as a company). */
export const LEAD_CHANNEL = '__lead'


/** Rows hanging off a trunk, each joined to it by a rounded branch; a branch
 * draws itself in the accent colour while that row is working. Measured from
 * the rows, since rows differ in height (a branch with its own reports). */
function Branches({ rows, indent, rowHeight }: { rows: Array<{ key: string; lit: boolean; head: ReactNode; body?: ReactNode }>; indent: number; rowHeight: number }) {
  const theme = useTheme()
  const box = useRef<HTMLDivElement>(null)
  const [ys, setYs] = useState<number[]>([])

  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    let frame = 0
    const measure = () => {
      const heads = [...el.querySelectorAll<HTMLElement>(':scope > [data-row] > [data-head]')]
      const next = heads.map((h) => Math.round(h.offsetTop + h.offsetHeight / 2))
      setYs((prev) => (prev.length === next.length && prev.every((y, i) => y === next[i]) ? prev : next))
    }
    measure()
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(measure)
    })
    ro.observe(el)
    return () => {
      cancelAnimationFrame(frame)
      ro.disconnect()
    }
  }, [rows.length])

  const trunk = 11
  const r = Math.min(7, rowHeight / 2 - 2)
  const endX = indent - 4
  const height = Math.max(0, ...ys.map((y) => y + rowHeight / 2))
  const branch = (y: number) => `M ${trunk} ${y - r} A ${r} ${r} 0 0 0 ${trunk + r} ${y} H ${endX}`
  const reach = (y: number) => `M ${trunk} 0 V ${y - r} A ${r} ${r} 0 0 0 ${trunk + r} ${y} H ${endX}`
  const length = (y: number) => y - r + (Math.PI * r) / 2 + (endX - trunk - r)
  const line = theme.palette.divider
  const accent = theme.palette.primary.main

  return (
    <Box ref={box} sx={{ position: 'relative' }}>
      {ys.length === rows.length && ys.length > 0 && (
        <svg width={indent} height={height} aria-hidden style={{ position: 'absolute', top: 0, left: 0, overflow: 'visible', pointerEvents: 'none' }}>
          <path d={`M ${trunk} 0 V ${ys[ys.length - 1] - r}`} fill="none" stroke={line} strokeWidth={1.5} strokeLinecap="round" />
          {ys.map((y, i) => (
            <path key={`b${rows[i].key}`} d={branch(y)} fill="none" stroke={line} strokeWidth={1.5} strokeLinecap="round" />
          ))}
          {ys.map((y, i) => (
            <path
              key={`r${rows[i].key}`}
              d={reach(y)}
              fill="none"
              stroke={accent}
              strokeWidth={1.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              style={{
                strokeDasharray: length(y),
                strokeDashoffset: rows[i].lit ? 0 : length(y),
                transition: 'stroke-dashoffset 400ms cubic-bezier(0.23, 1, 0.32, 1)',
              }}
            />
          ))}
        </svg>
      )}
      {rows.map((row) => (
        <Box key={row.key} data-row="" sx={{ pl: `${indent}px` }}>
          <Box data-head="" sx={{ minHeight: rowHeight, display: 'flex', alignItems: 'center' }}>
            {row.head}
          </Box>
          {row.body}
        </Box>
      ))}
    </Box>
  )
}

/**
 * A tree project in the sidebar: its groups, and the team as an org chart.
 * The head of the tree is the lead (the master, for a project made as a
 * company) or you; everyone else branches off whoever they report to. Picking
 * a group or a bot opens it in the chat panel.
 */
export function TreeTeam({
  root,
  team,
  master,
}: {
  root: string
  team: TeamAgent[]
  /** The project conversation's lead, when the tree was made as a company. */
  master: { name: string; color: string } | null
}) {
  const theme = useTheme()
  const status = useBotsStore((s) => s.status)
  const channel = useBotsStore((s) => s.channel[root] ?? 'room')
  const approvals = useBotsStore((s) => s.approvals)
  const save = useTeamStore((s) => s.save)
  const { setChannel, loadRoom, loadDm, loadStatus, loadGroups } = useBotsStore.getState()

  useEffect(() => {
    void loadRoom(root)
    void loadStatus(root)
    void loadGroups(root)
    for (const a of team) void loadDm(root, a.id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, team.map((a) => a.id).join(',')])

  const isWorking = (id: string) => status[botKey(root, id)]?.state === 'working'
  const known = new Set(team.map((a) => a.id))
  const boss = (a: TeamAgent) => (a.reportsTo && known.has(a.reportsTo) ? a.reportsTo : null)
  const under = (id: string | null) =>
    team.filter((a) => boss(a) === id).sort((x, y) => Number(!!y.lead) - Number(!!x.lead) || x.name.localeCompare(y.name))
  const anyBelowWorking = (id: string | null): boolean => under(id).some((a) => isWorking(a.id) || anyBelowWorking(a.id))

  const rowSx = {
    display: 'flex',
    flex: 1,
    minWidth: 0,
    justifyContent: 'flex-start',
    alignItems: 'center',
    gap: 1,
    textAlign: 'left',
    px: 0.75,
    py: 0.5,
    borderRadius: '8px',
    '&:hover': { bgcolor: 'action.hover' },
  } as const

  const botHead = (a: TeamAgent) => {
    const st = status[botKey(root, a.id)]
    const waiting = approvals.some((p) => p.root === root && p.botId === a.id)
    const state = st?.state ?? 'idle'
    const active = channel === a.id
    const subtitle = waiting ? 'Waiting for your approval' : state === 'working' ? (st?.activity ?? 'Working…') : a.role || 'Specialist'
    return (
      <Box sx={{ position: 'relative', flex: 1, minWidth: 0, display: 'flex', '&:hover .tree-star': { opacity: 1 } }}>
        <ButtonBase onClick={() => setChannel(root, a.id)} aria-current={active ? 'true' : undefined} sx={{ ...rowSx, pr: 3.5, bgcolor: active ? alpha(theme.palette.primary.main, 0.12) : undefined }}>
          <Box sx={{ position: 'relative', flexShrink: 0, display: 'flex' }}>
            <LiveAvatar root={root} agent={a} size={22} animate />
            {waiting && (
              <Box sx={{ position: 'absolute', right: -2, bottom: -2, width: 8, height: 8, borderRadius: '50%', border: 2, borderColor: 'background.paper', bgcolor: 'warning.main' }} />
            )}
          </Box>
          <Stack sx={{ minWidth: 0, flex: 1 }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: active ? 600 : 500, fontSize: '0.8125rem', lineHeight: 1.3 }} noWrap>
                {a.name}
              </Typography>
              {a.lead && (
                <Box component="span" sx={{ flexShrink: 0, px: 0.5, borderRadius: '4px', fontSize: '0.5625rem', fontWeight: 700, letterSpacing: 0.4, textTransform: 'uppercase', color: 'warning.main', bgcolor: alpha(theme.palette.warning.main, 0.14) }}>
                  Lead
                </Box>
              )}
            </Stack>
            <Typography variant="caption" color={waiting ? 'warning.main' : state === 'working' ? 'primary.main' : 'text.secondary'} sx={{ fontSize: '0.6875rem', lineHeight: 1.3 }} noWrap>
              {subtitle}
            </Typography>
          </Stack>
          {state === 'working' && <Orb state="working" px={14} label={`${a.name} is working`} />}
        </ButtonBase>
        <Tooltip title={a.lead ? 'Remove as lead' : 'Make lead: it hires into its branch and hands out its work'}>
          <IconButton
            size="small"
            className="tree-star"
            aria-label={a.lead ? `Remove ${a.name} as lead` : `Make ${a.name} a lead`}
            onClick={() => void save(root, { ...a, lead: !a.lead }, a.id)}
            sx={{ position: 'absolute', right: 2, top: '50%', transform: 'translateY(-50%)', opacity: 0, color: a.lead ? 'warning.main' : 'text.secondary', transition: 'opacity 0.12s' }}
          >
            <StarLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Box>
    )
  }

  const branchesUnder = (id: string | null, depth: number): ReactNode => {
    const reports = under(id)
    if (reports.length === 0) return undefined
    return (
      <Branches
        indent={depth === 0 ? 26 : 24}
        rowHeight={depth === 0 ? 38 : 34}
        rows={reports.map((a) => ({ key: a.id, lit: isWorking(a.id), head: botHead(a), body: branchesUnder(a.id, depth + 1) }))}
      />
    )
  }

  // The head of the tree: the master, or you.
  const headActive = channel === LEAD_CHANNEL
  const head = master ? (
    <ButtonBase onClick={() => setChannel(root, LEAD_CHANNEL)} aria-current={headActive ? 'true' : undefined} sx={{ ...rowSx, py: 0.75, bgcolor: headActive ? alpha(theme.palette.primary.main, 0.12) : undefined }}>
      <Box sx={{ width: 26, height: 26, borderRadius: '50%', flexShrink: 0, display: 'grid', placeItems: 'center', bgcolor: alpha(master.color, 0.2), color: master.color, fontSize: '0.75rem', fontWeight: 700 }}>
        {master.name.trim()[0]?.toUpperCase() ?? 'L'}
      </Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 650, fontSize: '0.8125rem', lineHeight: 1.3 }} noWrap>
          {master.name}
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem', lineHeight: 1.3 }} noWrap>
          Lead · delegates, checks, decides
        </Typography>
      </Stack>
      {anyBelowWorking(null) && <Orb state="working" px={14} />}
    </ButtonBase>
  ) : (
    <Stack direction="row" sx={{ ...rowSx, py: 0.75, cursor: 'default', '&:hover': undefined }}>
      <Box sx={{ width: 26, height: 26, borderRadius: '50%', flexShrink: 0, display: 'grid', placeItems: 'center', bgcolor: 'action.selected', color: 'text.secondary', fontSize: '0.6875rem', fontWeight: 700 }}>
        You
      </Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 650, fontSize: '0.8125rem', lineHeight: 1.3 }} noWrap>
          You
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem', lineHeight: 1.3 }} noWrap>
          {team.some((a) => a.lead) ? 'Above the leads' : 'You lead · star a bot to make it lead'}
        </Typography>
      </Stack>
    </Stack>
  )

  return (
    <Stack sx={{ px: 1.25, gap: 1.75 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.25 }}>
          <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
            Team{team.length ? ` · ${team.length}` : ''}
          </Typography>
        </Stack>
        {head}
        {branchesUnder(null, 0)}
        {team.length === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, py: 0.5, lineHeight: 1.45 }}>
            No one on the team yet. Ask the lead to hire someone.
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
