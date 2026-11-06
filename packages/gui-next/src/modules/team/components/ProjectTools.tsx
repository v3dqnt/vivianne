import { useState, type ReactNode } from 'react'
import { Box, ButtonBase, Collapse, Stack, Typography } from '../../../ui'
import { ChevronRightLinear } from '../../../icons'
import { WorkspaceSelector } from '../../ai/components/WorkspaceSelector'
import { GitStatusChip, GitQuickActions } from '../../git/components/GitActions'
import { useAppView } from '../../git/components/GitView'
import { ContextBreakdown, ContextPercent } from '../../ai/components/ContextUsageBar'
import { ThreadWorktree } from './ThreadWorktree'
import { ThreadPrs } from './ThreadPrs'
import { ProjectPill } from '../../sessions/components/ProjectPicker'
import { useDaysStore } from '../../sessions/store/daysStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'

/** Every action in the column is the same row: left-aligned on one edge,
 * one line, muted until hovered. */
const ROW = {
  display: 'flex',
  width: '100%',
  justifyContent: 'flex-start',
  alignItems: 'center',
  height: 28,
  px: '8px',
  borderRadius: '6px',
  fontSize: '0.8125rem',
  fontWeight: 400,
  color: 'text.secondary',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
} as const

/** The cards beside the chat: a flat surface one step up from the
 * workspace, set apart by a hairline rim. */
export const CARD = {
  borderRadius: '16px',
  bgcolor: 'action.hover',
  border: 1,
  borderColor: 'divider',
} as const

/** The rail's shape follows the window. In a normal window it is a narrow card, 17% of the width and
 * between 216px and 340px, with its sections stacked. In a big (maximised) window it widens into a
 * landscape card with the sections side by side. The chat container sets `--rail-w`; the rail and the
 * space the chat leaves for it both read it, so they always agree. */
export const RAIL_VARS = {
  '--rail-w': 'clamp(216px, 17vw, 340px)',
  '@media (min-width: 1700px)': { '--rail-w': 'clamp(520px, 32vw, 640px)' },
} as const
/** The chat leaves the rail plus a 24px gutter either side. */
export const RAIL_RESERVE = 'calc(var(--rail-w) + 48px)'

export const LABEL = { fontSize: '0.6875rem', fontWeight: 500, color: 'text.disabled', px: '8px', pb: '4px' } as const

const KEY = 'viv.projectRail.closed'
const readClosed = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? '[]')
  } catch {
    return []
  }
}

/** One collapsible section of the project rail: a title row (with something
 * quiet on its right) that folds the section away. Remembered between launches. */
function Section({ id, title, right, children }: { id: string; title: string; right?: ReactNode; children: ReactNode }) {
  const [closed, setClosed] = useState(() => readClosed().includes(id))
  const toggle = () => {
    const next = !closed
    setClosed(next)
    try {
      const rest = readClosed().filter((x) => x !== id)
      localStorage.setItem(KEY, JSON.stringify(next ? [...rest, id] : rest))
    } catch {
      /* not remembered */
    }
  }
  return (
    <Box>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '4px', pb: '4px' }}>
        <ButtonBase
          onClick={toggle}
          aria-expanded={!closed}
          sx={{ flex: '1 0 auto', justifyContent: 'flex-start', gap: '4px', px: '8px', height: 20, borderRadius: '6px', color: 'text.disabled', '& .chev': { opacity: closed ? 1 : 0, transition: 'opacity 0.15s' }, '&:hover': { color: 'text.primary' }, '&:hover .chev': { opacity: 1 } }}
        >
          <Box className="chev" sx={{ display: 'flex', transform: closed ? 'none' : 'rotate(90deg)', transition: 'transform 0.15s ease, opacity 0.15s' }}>
            <ChevronRightLinear size={12} color="currentColor" />
          </Box>
          <Typography noWrap sx={{ fontSize: '0.6875rem', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'inherit', flexShrink: 0 }}>{title}</Typography>
        </ButtonBase>
        <Box sx={{ minWidth: 0, flexShrink: 1, overflow: 'hidden', pr: '8px' }}>{right}</Box>
      </Stack>
      <Collapse in={!closed} unmountOnExit>
        {children}
      </Collapse>
    </Box>
  )
}

/** The project rail, on the chat's right: the folder the project works in,
 * version control and the context window. */
export function ProjectTools() {
  const todayId = useDaysStore((s) => s.todayId)
  const hasFolder = useProjectsStore((s) => {
    const id = todayId ? s.sessionProjectMap[todayId] : undefined
    return !!s.projects.find((p) => p.id === id)?.path
  })
  return (
    <Box
      sx={{
        position: 'absolute',
        right: 24,
        top: 24,
        width: 'var(--rail-w)',
        containerType: 'inline-size',
        zIndex: 3,
        p: '16px 14px',
        ...CARD,
      }}
    >
      {/* Stacked in the narrow card, side by side in the landscape one: the card asks its
          own width, not the window's. */}
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'minmax(0, 1fr)',
          gap: '12px',
          '& > .col + .col, & .git, & .wt': { borderTop: 1, borderColor: 'divider', pt: '12px' },
          '@container (min-width: 500px)': {
            display: 'flex',
            '& > .col': { flex: '1 1 0', minWidth: 0 },
            '& > .col:last-child': { flex: '1.2 1 0' },
            '& .git, & .wt': { borderTop: 0, pt: 0 },
            columnGap: '20px',
            '& > .col + .col': { borderTop: 0, pt: 0, borderLeft: 1, borderColor: 'divider', pl: '20px' },
          },
        }}
      >
        <Box className="col" sx={{ display: 'flex', flexDirection: 'column', gap: '12px', minWidth: 0 }}>
          {/* The project and its folders are one thing: the project's name, with the
              folders it works in managed from the row under it. */}
          <Section id="project" title="Project">
            <ProjectPill variant="row" />
            {hasFolder && (
              <Box sx={{ ml: '-2px' }}>
                <WorkspaceSelector title="Folders and access" />
              </Box>
            )}
          </Section>
          {hasFolder && <ThreadWorktree />}
          {hasFolder && (
          <Box className="git" sx={{ minWidth: 0 }}>
            <Section
              id="git"
              title="Version control"
              right={
                <Box sx={{ minWidth: 0, '& .ui-ButtonBase': { height: 20, px: '4px' } }}>
                  <GitStatusChip />
                </Box>
              }
            >
              {/* The quick actions (commit, sync, pull request), drawn as the column's
                  rows rather than as their own buttons. */}
              <Box
                sx={{
                  '& > div': { flexDirection: 'column', alignItems: 'stretch', gap: 0 },
                  '& > div > span:not(:has(button))': { display: 'none' },
                  '& .ui-Button': {
                    ...ROW,
                    border: 0,
                    bgcolor: 'transparent',
                    boxShadow: 'none',
                    transform: 'none',
                    '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
                  },
                  '& .ui-Button:first-of-type': { color: 'text.primary', fontWeight: 500 },
                }}
              >
                <GitQuickActions />
              </Box>
              <ThreadPrs />
              <ButtonBase onClick={() => useAppView.setState({ view: 'git' })} sx={ROW}>
                History and PRs
              </ButtonBase>
            </Section>
          </Box>
          )}
        </Box>

        <Box className="col" sx={{ minWidth: 0 }}>
          <Section id="context" title="Context" right={<ContextPercent />}>
            <ContextBreakdown />
          </Section>
        </Box>
      </Box>
    </Box>
  )
}
