import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Box, ButtonBase, IconButton, Orb, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { AddLinear, ChevronRightLinear, DocumentTextLinear } from '../../../icons'
import { Avatar, colorOf } from './bits'
import { isWorking, type AgentRun, type ChildRun, type TeamAgent } from '../store/teamStore'

/**
 * The company as a tree: the project lead at the top, the employees branching
 * off it, and the subagents each employee spawned branching off them.
 *
 * The lead is who you talk to: clicking it opens its conversation, where you
 * say what you want and it routes each piece to the employee whose area it is
 * (`agent::team::prompt_section`). A branch draws itself in while that
 * employee is working, so you can see at a glance where the work went.
 *
 * Drawn after the "branched menu" pattern: a trunk with rounded branches to
 * each row, measured from the rows themselves since rows differ in height.
 */

/** Subagents stay on the tree this long after they finish. */
const RECENT_MS = 15 * 60 * 1000
const MAX_CHILDREN = 5

interface Row {
  key: string
  /** Lit: the branch to this row is drawn in the accent colour. */
  lit: boolean
  head: ReactNode
  /** Under the row's head, inside its branch (its own sub-branches). */
  body?: ReactNode
}

/** Rows hanging off a trunk, each joined to it by a rounded branch. */
function Branches({ rows, indent, rowHeight, accent }: { rows: Row[]; indent: number; rowHeight: number; accent: string }) {
  const theme = useTheme()
  const box = useRef<HTMLDivElement>(null)
  const [ys, setYs] = useState<number[]>([])

  // Where each row's head is, for the branches. Re-measured when rows change
  // size (a nested tree opening), on the next frame, so that drawing the
  // lines never resizes what is being observed within the same frame.
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

  const trunk = 9
  const r = Math.min(8, rowHeight / 2 - 2)
  const endX = indent - 6
  const height = Math.max(0, ...ys.map((y) => y + rowHeight / 2))
  const branch = (y: number) => `M ${trunk} ${y - r} A ${r} ${r} 0 0 0 ${trunk + r} ${y} H ${endX}`
  const reach = (y: number) => `M ${trunk} 0 V ${y - r} A ${r} ${r} 0 0 0 ${trunk + r} ${y} H ${endX}`
  const length = (y: number) => y - r + (Math.PI * r) / 2 + (endX - trunk - r)
  const line = theme.palette.divider

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

function rowSx(theme: ReturnType<typeof useTheme>) {
  return {
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
    '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 1 },
  } as const
}

/** The subagents worth showing for an employee: running, or just finished. */
function visibleChildren(runs: AgentRun[] | undefined, now: number): ChildRun[] {
  const all = (runs ?? []).flatMap((r) => r.children ?? [])
  return all
    .filter((c) => c.status === 'running' || (c.endedAt !== null && now - c.endedAt < RECENT_MS))
    .sort((a, b) => b.startedAt - a.startedAt)
    .slice(0, MAX_CHILDREN)
}

const CHILD_DOT: Record<ChildRun['status'], string> = {
  running: 'primary.main',
  completed: 'success.main',
  failed: 'error.main',
  cancelled: 'text.disabled',
  interrupted: 'text.disabled',
}

export function CompanyTree({
  leadName,
  leadColor,
  leadActive,
  team,
  runs,
  pendingHires,
  onLead,
  onCharter,
  onAgent,
  onManage,
}: {
  leadName: string
  leadColor: string
  /** The lead's conversation is the one open. */
  leadActive: boolean
  team: TeamAgent[]
  /** `agentId` → its runs, newest first. */
  runs: (agentId: string) => AgentRun[] | undefined
  pendingHires: number
  onLead: () => void
  onCharter: () => void
  onAgent: (id: string | null) => void
  onManage: () => void
}) {
  const theme = useTheme()
  const [open, setOpen] = useState(true)
  const now = Date.now()
  const accent = theme.palette.primary.main
  const anyWorking = team.some((a) => isWorking(runs(a.id)))

  const employeeRows: Row[] = team.map((a) => {
    const agentRuns = runs(a.id)
    const working = isWorking(agentRuns)
    const current = agentRuns?.find((r) => r.status === 'running')
    const kids = visibleChildren(agentRuns, now)
    return {
      key: a.id,
      lit: working,
      head: (
        <ButtonBase onClick={() => onAgent(a.id)} sx={rowSx(theme)}>
          <Box sx={{ position: 'relative', flexShrink: 0, display: 'flex' }}>
            <Avatar name={a.name} color={colorOf(a)} size={20} />
          </Box>
          <Stack sx={{ minWidth: 0, flex: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 500, fontSize: '0.8125rem', lineHeight: 1.3 }} noWrap>
              {a.name}
            </Typography>
            <Typography variant="caption" color={working ? 'primary.main' : 'text.secondary'} sx={{ fontSize: '0.6875rem', lineHeight: 1.3 }} noWrap>
              {working ? current?.description || 'Working…' : a.role || 'Idle'}
            </Typography>
          </Stack>
          {working && <Orb state="working" px={16} label={`${a.name} is working`} />}
        </ButtonBase>
      ),
      body:
        kids.length > 0 ? (
          <Branches
            indent={26}
            rowHeight={28}
            accent={accent}
            rows={kids.map((c) => ({
              key: c.id,
              lit: c.status === 'running',
              head: (
                <ButtonBase onClick={() => onAgent(a.id)} sx={{ ...rowSx(theme), py: 0.25, gap: 0.75 }} title={c.description}>
                  {c.status === 'running' ? (
                    <Orb state="searching" px={14} label="Subagent working" />
                  ) : (
                    <Box sx={{ width: 6, height: 6, mx: '4px', borderRadius: '50%', flexShrink: 0, bgcolor: CHILD_DOT[c.status] }} />
                  )}
                  <Typography variant="caption" sx={{ fontSize: '0.71875rem', color: c.status === 'running' ? 'text.primary' : 'text.secondary', minWidth: 0 }} noWrap>
                    <Box component="span" sx={{ fontWeight: 600 }}>
                      {c.kind}
                    </Box>
                    {c.description ? ` · ${c.description}` : ''}
                  </Typography>
                </ButtonBase>
              ),
            }))}
          />
        ) : undefined,
    }
  })

  const extraRows: Row[] = [
    ...(pendingHires > 0
      ? [
          {
            key: '__hires',
            lit: false,
            head: (
              <ButtonBase onClick={onManage} sx={{ ...rowSx(theme), bgcolor: alpha(accent, 0.1), color: 'primary.main', '&:hover': { bgcolor: alpha(accent, 0.16) } }}>
                <Typography variant="caption" sx={{ fontWeight: 600, fontSize: '0.75rem' }}>
                  {pendingHires} proposed hire{pendingHires === 1 ? '' : 's'} to review
                </Typography>
              </ButtonBase>
            ),
          },
        ]
      : []),
    {
      key: '__add',
      lit: false,
      head: (
        <ButtonBase onClick={onManage} sx={{ ...rowSx(theme), color: 'text.secondary', gap: 0.75 }}>
          <AddLinear size={13} color="currentColor" />
          <Typography variant="caption" sx={{ fontSize: '0.75rem' }}>
            {team.length ? 'Add an employee' : `No employees yet — ${leadName} proposes them as the work needs, or add one`}
          </Typography>
        </ButtonBase>
      ),
    },
  ]

  return (
    <Box
      sx={{
        position: 'relative',
        pl: '12px',
        // The company's spine: fades out below, like the branched menu's.
        '&::before': {
          content: '""',
          position: 'absolute',
          top: 8,
          bottom: 0,
          left: 0,
          width: 2,
          borderRadius: 1,
          background: `linear-gradient(to bottom, ${theme.palette.divider} 0%, ${theme.palette.divider} 55%, transparent 100%)`,
        },
      }}
    >
      {/* Marks the lead while its conversation is the one open. */}
      <Box
        aria-hidden
        sx={{
          position: 'absolute',
          left: 0,
          top: 14,
          width: 2,
          height: 16,
          borderRadius: 1,
          bgcolor: 'text.primary',
          opacity: leadActive ? 1 : 0,
          transition: 'opacity 150ms ease',
          zIndex: 1,
        }}
      />

      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25 }}>
        <Tooltip title={`Tell ${leadName} what you want — it routes each piece to the right employee`} placement="right">
          <ButtonBase onClick={onLead} sx={{ ...rowSx(theme), py: 0.75, bgcolor: leadActive ? 'action.selected' : undefined }}>
            <Box
              sx={{
                width: 24,
                height: 24,
                borderRadius: '50%',
                flexShrink: 0,
                display: 'grid',
                placeItems: 'center',
                bgcolor: alpha(leadColor, 0.2),
                color: leadColor,
                fontSize: '0.75rem',
                fontWeight: 700,
              }}
            >
              {leadName.trim()[0]?.toUpperCase() ?? 'L'}
            </Box>
            <Stack sx={{ minWidth: 0, flex: 1 }}>
              <Typography variant="body2" sx={{ fontWeight: 650, fontSize: '0.8125rem', lineHeight: 1.3 }} noWrap>
                {leadName}
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem', lineHeight: 1.3 }} noWrap>
                Project lead · tell it your goals
              </Typography>
            </Stack>
            {anyWorking && !open && <Orb state="working" px={16} />}
          </ButtonBase>
        </Tooltip>
        <Tooltip title="Charter">
          <IconButton size="small" aria-label="Open the charter" onClick={onCharter} sx={{ color: 'text.secondary' }}>
            <DocumentTextLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title={open ? 'Fold the team' : 'Show the team'}>
          <IconButton size="small" aria-label={open ? 'Fold the team' : 'Show the team'} aria-expanded={open} onClick={() => setOpen((v) => !v)} sx={{ color: 'text.secondary' }}>
            <Box sx={{ display: 'flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 200ms ease' }}>
              <ChevronRightLinear size={13} color="currentColor" />
            </Box>
          </IconButton>
        </Tooltip>
      </Stack>

      {/* Folds with a grid-rows transition, so the height animates to its content. */}
      <Box sx={{ display: 'grid', gridTemplateRows: open ? '1fr' : '0fr', transition: 'grid-template-rows 300ms cubic-bezier(0.23, 1, 0.32, 1)' }}>
        <Box sx={{ minHeight: 0, overflow: 'hidden' }}>
          <Box sx={{ pt: 0.5, opacity: open ? 1 : 0, transition: 'opacity 200ms ease' }}>
            <Branches indent={30} rowHeight={40} accent={accent} rows={[...employeeRows, ...extraRows]} />
          </Box>
        </Box>
      </Box>
    </Box>
  )
}
