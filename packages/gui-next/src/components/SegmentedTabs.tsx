import type { ReactNode } from 'react'
import { Box, ButtonBase, Stack } from '../ui'

export interface SegmentedTab<T extends string> {
  id: T
  label: string
  icon?: ReactNode
}

/**
 * The app's segmented tabs: a muted track with the chosen tab raised on a paper chip. One component, so Home's Projects / 24/7 agents
 * switch and the project rail's tabs look and behave the same. `fill` stretches the tabs to share the track's width; `compact` shows
 * icons only (the label stays as the accessible name and the tooltip).
 */
export function SegmentedTabs<T extends string>({ tabs, value, onChange, fill = false, compact = false }: { tabs: SegmentedTab<T>[]; value: T; onChange: (id: T) => void; fill?: boolean; compact?: boolean }) {
  return (
    <Stack direction="row" role="tablist" sx={{ gap: '4px', p: '2px', borderRadius: '8px', bgcolor: 'action.hover', alignSelf: fill ? 'stretch' : 'flex-start' }}>
      {tabs.map(({ id, label, icon }) => {
        const active = id === value
        return (
          <ButtonBase
            key={id}
            role="tab"
            aria-selected={active}
            aria-label={label}
            title={compact ? label : undefined}
            onClick={() => onChange(id)}
            sx={{
              flex: fill ? '1 1 0' : '0 0 auto',
              minWidth: 0,
              height: 28,
              px: compact ? '8px' : '12px',
              gap: '6px',
              borderRadius: '6px',
              fontSize: '0.8125rem',
              fontWeight: active ? 600 : 500,
              color: active ? 'text.primary' : 'text.secondary',
              bgcolor: active ? 'background.paper' : 'transparent',
              transition: 'background-color 160ms, color 160ms',
              '&:hover': { color: 'text.primary' },
            }}
          >
            {icon && <Box sx={{ display: 'flex', flexShrink: 0 }}>{icon}</Box>}
            {!compact && (
              <Box component="span" sx={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {label}
              </Box>
            )}
          </ButtonBase>
        )
      })}
    </Stack>
  )
}
