import { useEffect } from 'react'

/**
 * Clears the document's own background so a `transparent: true` Tauri window
 * actually renders transparent.
 *
 * Every window loads the same bundle, so MUI's `CssBaseline` paints
 * `background.default` onto `<body>` in all of them. That is exactly right for
 * the main window and exactly wrong for the overlay windows: the notch is
 * meant to be a floating pill and the DIP overlay is meant to be invisible
 * except for its highlight boxes, but both were rendering as opaque
 * rectangles the size of the whole window.
 *
 * The pre-rewrite notch did this too (`document.body.style.background =
 * "transparent"`); the port to `gui-next` dropped it.
 *
 * Applied as an effect rather than at module scope because it must run after
 * `CssBaseline` has injected its own styles — otherwise it is overwritten on
 * mount.
 */
export function useTransparentWindowBody(): void {
  useEffect(() => {
    const { body, documentElement } = document
    const prev = {
      bodyBg: body.style.background,
      bodyMinHeight: body.style.minHeight,
      htmlBg: documentElement.style.background,
    }
    body.style.background = 'transparent'
    // The app shell stretches body to fill the viewport; an overlay window is
    // sized to its content instead.
    body.style.minHeight = '0'
    documentElement.style.background = 'transparent'
    return () => {
      body.style.background = prev.bodyBg
      body.style.minHeight = prev.bodyMinHeight
      documentElement.style.background = prev.htmlBg
    }
  }, [])
}
