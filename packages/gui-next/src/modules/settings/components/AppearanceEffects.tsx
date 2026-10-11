import { useEffect } from 'react'
import { useThemeStore, type ChatWidth, type Density } from '../store/themeStore'

const CHAT_WIDTH: Record<ChatWidth, string> = {
  narrow: 'min(82%, 600px)',
  normal: 'min(82%, 720px)',
  wide: 'min(90%, 960px)',
  full: '100%',
}

/** Chat type sizes and card header height per density. */
const DENSITY: Record<Density, { text: string; title: string; meta: string; header: string; gap: string }> = {
  compact: { text: '0.8125rem', title: '0.75rem', meta: '0.6875rem', header: '32px', gap: '6px' },
  comfortable: { text: '0.875rem', title: '0.8125rem', meta: '0.75rem', header: '38px', gap: '10px' },
  roomy: { text: '0.9375rem', title: '0.875rem', meta: '0.8125rem', header: '44px', gap: '14px' },
}

/**
 * Applies what the theme object doesn't carry: whole-app scale, the chat's
 * density and width (CSS variables `ai/lib/chatStyle.ts` reads), and the
 * wallpaper with its glass. Loads custom themes and the wallpaper once.
 */
export function AppearanceEffects() {
  const uiScale = useThemeStore((s) => s.uiScale)
  const density = useThemeStore((s) => s.density)
  const chatWidth = useThemeStore((s) => s.chatWidth)
  const wallpaper = useThemeStore((s) => s.wallpaper)
  const dim = useThemeStore((s) => s.wallpaperDim)
  const glass = useThemeStore((s) => s.glass)
  const loadCustom = useThemeStore((s) => s.loadCustom)
  const loadWallpaper = useThemeStore((s) => s.loadWallpaper)

  useEffect(() => {
    void loadCustom()
    void loadWallpaper()
  }, [loadCustom, loadWallpaper])

  useEffect(() => {
    const root = document.documentElement
    // `zoom` scales layout, text and hit-testing together (WebView2 and
    // WebKit both support it), unlike a root font-size, which only reaches
    // rem-sized things.
    root.style.setProperty('zoom', uiScale === 1 ? '' : String(uiScale))
  }, [uiScale])

  useEffect(() => {
    const root = document.documentElement.style
    const d = DENSITY[density]
    root.setProperty('--chat-max-width', CHAT_WIDTH[chatWidth])
    root.setProperty('--chat-font-text', d.text)
    root.setProperty('--chat-font-title', d.title)
    root.setProperty('--chat-font-meta', d.meta)
    root.setProperty('--chat-header-height', d.header)
    root.setProperty('--chat-gap', d.gap)
  }, [density, chatWidth])

  useEffect(() => {
    const body = document.body.style
    const root = document.documentElement.style
    if (!wallpaper) {
      body.backgroundImage = ''
      root.removeProperty('--viv-glass-filter')
      return
    }
    body.backgroundImage = `linear-gradient(rgba(0,0,0,${dim}), rgba(0,0,0,${dim})), url("${wallpaper}")`
    body.backgroundSize = 'cover'
    body.backgroundPosition = 'center'
    body.backgroundAttachment = 'fixed'
    root.setProperty('--viv-glass-filter', glass > 0 ? `blur(${Math.round(8 + glass * 22)}px) saturate(1.15)` : 'none')
  }, [wallpaper, dim, glass])

  return null
}
