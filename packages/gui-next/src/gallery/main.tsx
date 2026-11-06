import { StrictMode, Suspense, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Box, Button, CssBaseline, Stack, ThemeProvider, Typography } from '../ui'
import { baselineExtras, buildTheme } from '../theme'
import { KitSection } from './KitSection'
import { IconsSection } from './IconsSection'
import { AppSection } from './AppSection'
import { useThemeStore, themeOptions, activePaletteId } from '../modules/settings/store/themeStore'
import { AppearanceEffects } from '../modules/settings/components/AppearanceEffects'
import '../tailwind.css'

const TABS = ['Kit', 'Icons', 'App components'] as const

function Gallery() {
  // The app's own appearance pipeline, so every component is drawn exactly as in the app: its palette, fonts, radius, density and wallpaper.
  const appearance = useThemeStore()
  const mode = appearance.mode
  const setMode = appearance.setMode
  const [tab, setTab] = useState<(typeof TABS)[number]>('Kit')
  const paletteId = activePaletteId(appearance)
  const opts = themeOptions(appearance)
  const key = JSON.stringify([mode, paletteId, opts])
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const theme = useMemo(() => buildTheme(mode, paletteId, opts), [key])
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline extra={baselineExtras(mode)} />
      <AppearanceEffects />
      <Box sx={{ height: '100vh', overflow: 'auto', bgcolor: 'background.default', color: 'text.primary', p: '24px' }}>
        <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: '16px', gap: '12px', flexWrap: 'wrap' }}>
          <Typography variant="h5">Vivianne components</Typography>
          <Stack direction="row" sx={{ gap: '8px' }}>
            {TABS.map((t) => <Button key={t} variant={t === tab ? 'contained' : 'outlined'} size="small" onClick={() => setTab(t)}>{t}</Button>)}
            <Button size="small" variant="outlined" onClick={() => setMode(mode === 'dark' ? 'light' : 'dark')}>{mode === 'dark' ? 'Light' : 'Dark'}</Button>
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
