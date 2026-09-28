import { createContext, useContext, type ReactNode } from 'react'
import { Global, ThemeProvider as EmotionThemeProvider } from '@emotion/react'
import { Tooltip as RadixTooltip } from 'radix-ui'
import type { Theme } from './theme'
import { resolve, type SxProps } from './sx'

const ThemeContext = createContext<Theme | null>(null)

/** The current theme. Throws outside a `ThemeProvider`. */
export function useTheme(): Theme {
  const theme = useContext(ThemeContext)
  if (!theme) throw new Error('useTheme() called outside <ThemeProvider>')
  return theme
}

/** Resolve an `sx` value against the current theme. */
export function useSx(sx: SxProps | undefined): ReturnType<typeof resolve> {
  return resolve(sx, useTheme())
}

/**
 * Provides the theme to every component (and to Emotion), and the shared
 * Radix Tooltip provider — one per app so moving between tooltips skips the
 * open delay, like hovering along a toolbar.
 */
export function ThemeProvider({ theme, children }: { theme: Theme; children: ReactNode }) {
  return (
    <ThemeContext.Provider value={theme}>
      <EmotionThemeProvider theme={theme}>
        <RadixTooltip.Provider delayDuration={100} skipDelayDuration={300}>
          {children}
        </RadixTooltip.Provider>
      </EmotionThemeProvider>
    </ThemeContext.Provider>
  )
}

/** Global styles, with `sx`-style theme mapping. */
export function GlobalStyles({ styles }: { styles: SxProps }) {
  const theme = useTheme()
  // biome-ignore lint/suspicious/noExplicitAny: Emotion's own Theme type is empty
  return <Global styles={resolve(styles, theme) as any} />
}

/**
 * The app's reset: box-sizing, font smoothing, the body's type and colors.
 * The theme-specific extras (no root scrolling, thin scrollbars) are passed
 * in by the caller.
 */
export function CssBaseline({ extra }: { extra?: Record<string, unknown> }) {
  const theme = useTheme()
  const p = theme.palette
  return (
    <Global
      styles={[
        {
          html: {
            WebkitFontSmoothing: 'antialiased',
            MozOsxFontSmoothing: 'grayscale',
            boxSizing: 'border-box',
            WebkitTextSizeAdjust: '100%',
            colorScheme: p.mode,
          },
          '*, *::before, *::after': { boxSizing: 'inherit' },
          // Emphasis inside content (chat, docs) only; chrome never goes bold.
          'strong, b': { fontWeight: 600 },
          body: {
            margin: 0,
            color: p.text.primary,
            ...theme.typography.body1,
            backgroundColor: p.background.default,
            '@media print': { backgroundColor: p.common.white },
            '&::backdrop': { backgroundColor: p.background.default },
          },
        },
        // biome-ignore lint/suspicious/noExplicitAny: plain style object
        (extra ?? {}) as any,
      ]}
    />
  )
}
