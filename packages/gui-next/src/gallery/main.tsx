import { StrictMode, Suspense, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Box, Button, CssBaseline, Stack, ThemeProvider, Typography } from '../ui'
import { baselineExtras, buildTheme } from '../theme'
import { KitSection } from './KitSection'
import { IconsSection } from './IconsSection'
import { AppSection } from './AppSection'
import '../tailwind.css'

const TABS = ['Kit', 'Icons', 'App components'] as const

function Gallery() {
  const [mode, setMode] = useState<'light' | 'dark'>('dark')
  const [tab, setTab] = useState<(typeof TABS)[number]>('Kit')
  const theme = useMemo(() => buildTheme(mode, 'neutral'), [mode])
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline extra={baselineExtras(mode)} />
      <Box sx={{ height: '100vh', overflow: 'auto', bgcolor: 'background.default', color: 'text.primary', p: '24px' }}>
        <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: '16px', gap: '12px', flexWrap: 'wrap' }}>
          <Typography variant="h5">Vivianne components</Typography>
          <Stack direction="row" sx={{ gap: '8px' }}>
            {TABS.map((t) => <Button key={t} variant={t === tab ? 'contained' : 'outlined'} size="small" onClick={() => setTab(t)}>{t}</Button>)}
            <Button size="small" variant="outlined" onClick={() => setMode((m) => (m === 'dark' ? 'light' : 'dark'))}>{mode === 'dark' ? 'Light' : 'Dark'}</Button>
          </Stack>
        </Stack>
        <Suspense fallback={null}>
          {tab === 'Kit' && <KitSection />}
          {tab === 'Icons' && <IconsSection />}
          {tab === 'App components' && <AppSection />}
        </Suspense>
      </Box>
    </ThemeProvider>
  )
}

createRoot(document.getElementById('root')!).render(<StrictMode><Gallery /></StrictMode>)
