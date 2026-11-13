import type { ReactNode } from 'react'
import { Box, Stack, Typography } from '../ui'

/** A place with nothing in it yet: what belongs here, and the way to add the first one. */
export function EmptyState({ icon, title, hint, action }: { icon: ReactNode; title: string; hint?: string; action?: ReactNode }) {
  return (
    <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: '12px', py: '56px', px: '24px', borderRadius: '12px', border: 1, borderStyle: 'dashed', borderColor: 'divider' }}>
      <Box sx={{ display: 'grid', placeItems: 'center', width: 40, height: 40, borderRadius: '12px', bgcolor: 'action.hover', color: 'text.secondary' }}>{icon}</Box>
      <Stack sx={{ gap: '4px', maxWidth: 360 }}>
        <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>{title}</Typography>
        {hint && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', lineHeight: 1.5 }}>{hint}</Typography>}
      </Stack>
      {action}
    </Stack>
  )
}
