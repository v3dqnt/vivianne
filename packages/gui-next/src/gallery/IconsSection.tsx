import { createElement, type ComponentType } from 'react'
import { Box, Typography } from '../ui'
import * as icons from '../icons'

/** Every icon the app uses, by its name. */
export function IconsSection() {
  const all = Object.entries(icons).filter(([, v]) => typeof v === 'function') as [string, ComponentType<{ size?: number }>][]
  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))', gap: '8px' }}>
      {all.map(([name, Icon]) => (
        <Box key={name} sx={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px', p: '12px', borderRadius: '8px', border: 1, borderColor: 'divider' }}>
          {createElement(Icon, { size: 24 })}
          <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary', wordBreak: 'break-all', textAlign: 'center' }}>{name}</Typography>
        </Box>
      ))}
      <Typography sx={{ gridColumn: '1 / -1', fontSize: '0.75rem', color: 'text.secondary' }}>{all.length} icons</Typography>
    </Box>
  )
}
