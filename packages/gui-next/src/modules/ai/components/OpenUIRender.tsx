import { useContext, useMemo } from 'react'
import { Renderer, type ActionEvent } from '@openuidev/react-lang'
import { ThemeProvider } from '@openuidev/react-ui'
import { openuiChatLibrary } from '@openuidev/react-ui/genui-lib'
// Layered: every rule sits in `@layer openui`, so the app's own CSS always
// wins and nothing here can restyle the rest of the window.
import '@openuidev/react-ui/layered/styles/index.css'
import { useThemeStore } from '../../settings/store/themeStore'
import { FollowUpContext } from './InteractiveBlock'

/**
 * An OpenUI card (openui-lang, rendered with OpenUI's chat component
 * library). Loaded on first use: the library and its stylesheet are large.
 *
 * Buttons that talk back (`@ToAssistant`) send their message in the
 * conversation the card is in (FollowUpContext), as if the user typed it;
 * `@OpenUrl` opens the page in the browser.
 */
export default function OpenUIRender({ code, streaming }: { code: string; streaming: boolean }) {
  const mode = useThemeStore((s) => s.mode)
  const followUp = useContext(FollowUpContext)
  const onAction = useMemo(
    () => (e: ActionEvent) => {
      if (e.type === 'open_url') {
        const url = typeof e.params.url === 'string' ? e.params.url : ''
        if (/^https?:\/\//i.test(url)) window.open(url, '_blank', 'noopener,noreferrer')
        return
      }
      if (e.type === 'continue_conversation') {
        const text = (typeof e.params.message === 'string' && e.params.message) || e.humanFriendlyMessage
        if (text && followUp) followUp(text)
      }
    },
    [followUp],
  )
  return (
    <ThemeProvider mode={mode === 'dark' ? 'dark' : 'light'}>
      <div className="viv-openui" style={{ margin: '8px 0', maxWidth: 760 }}>
        <Renderer response={code} library={openuiChatLibrary} isStreaming={streaming} onAction={onAction} />
      </div>
    </ThemeProvider>
  )
}
