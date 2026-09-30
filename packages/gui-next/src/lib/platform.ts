/**
 * Host-platform detection for the few places the UI must say something
 * OS-specific (path separators, the name of the terminal app).
 *
 * Read from the webview's user agent rather than `@tauri-apps/plugin-os`
 * because it needs no plugin, no capability entry, and no async round trip —
 * these are render-time string choices, and an awaited value would flash the
 * wrong label first. Every WebView2 UA contains "Windows", every WKWebView UA
 * on macOS contains "Mac OS X".
 */
const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent

export const isMac = /Mac OS X|Macintosh/.test(ua)
export const isWindows = /Windows/.test(ua)

/** The separator to *display* in a path. Never use this to build a path that
 * goes back to the backend — send what the backend gave you, unmodified. */
export const pathSeparator = isWindows ? '\u005c' : '/'

/** What the OS calls the thing "open in terminal" opens. */
export const terminalAppName = isWindows ? 'PowerShell' : isMac ? 'Terminal' : 'a terminal'

/** What the OS calls its file manager. */
export const fileManagerName = isWindows ? 'File Explorer' : isMac ? 'Finder' : 'the file manager'
