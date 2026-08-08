import { createTheme } from '@mui/material/styles'
import './assets/fonts/fonts.css'

/**
 * The single source of visual truth. The previous frontend accumulated four
 * overlapping styling systems (Tailwind + daisyUI + Astryx + Radix), which is
 * what made it unmaintainable — every component picked a different one. This
 * app has exactly one: MUI's theme. No utility classes, no second component
 * library, no raw hex outside this file.
 */
export const theme = createTheme({
  palette: {
    mode: 'dark',
    background: {
      default: '#0a0a0a',
      paper: '#141414',
    },
    primary: {
      main: '#4a9eff',
    },
    text: {
      primary: '#ededed',
      secondary: '#a1a1a1',
    },
    divider: 'rgba(255,255,255,0.09)',
  },
  shape: {
    borderRadius: 10,
  },
  typography: {
    fontFamily: '"Sora", system-ui, -apple-system, sans-serif',
    fontSize: 14,
    button: {
      textTransform: 'none',
      fontWeight: 500,
    },
  },
  components: {
    // A desktop app is a single long-lived window, not a document — it should
    // never rubber-band or show a page scrollbar at the root.
    MuiCssBaseline: {
      styleOverrides: {
        'html, body, #root': {
          height: '100%',
          overflow: 'hidden',
          margin: 0,
        },
        // Custom scrollbar everywhere (WebView2 is Chromium-based, so the
        // `::-webkit-scrollbar` family works app-wide, not just Safari/Chrome).
        '*': {
          scrollbarWidth: 'thin',
          scrollbarColor: 'rgba(255,255,255,0.18) transparent',
        },
        '*::-webkit-scrollbar': {
          width: 8,
          height: 8,
        },
        '*::-webkit-scrollbar-track': {
          background: 'transparent',
        },
        '*::-webkit-scrollbar-thumb': {
          backgroundColor: 'rgba(255,255,255,0.14)',
          borderRadius: 8,
          border: '2px solid transparent',
          backgroundClip: 'padding-box',
        },
        '*::-webkit-scrollbar-thumb:hover': {
          backgroundColor: 'rgba(255,255,255,0.24)',
          border: '1px solid transparent',
          backgroundClip: 'padding-box',
        },
      },
    },
  },
})
