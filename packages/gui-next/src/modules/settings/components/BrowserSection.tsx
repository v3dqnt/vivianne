import { useEffect, useState } from 'react'
import { Box, Button, Stack, Typography } from '../../../ui'
import { call, on } from '../../../ipc'

/** Mirrors `helium::HeliumStatus`. */
interface HeliumStatus {
  connected: boolean
  browser: string | null
  browsers?: string[]
  extensionPath: string | null
}

/** Mirrors `helium::InstalledBrowser`. */
interface InstalledBrowser {
  id: string
  name: string
  path: string
  extensionsUrl: string
  connected: boolean
}

/**
 * The user's own browser: whether the Vivianne extension is connected, and
 * how to install it (Load unpacked from the folder Vivianne keeps current).
 */
export function BrowserSection() {
  const [status, setStatus] = useState<HeliumStatus | null>(null)
  const [copied, setCopied] = useState(false)
  const [browsers, setBrowsers] = useState<InstalledBrowser[]>([])
  const [opening, setOpening] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refresh = () => void call<InstalledBrowser[]>('cmd_chromium_browsers').then((b) => setBrowsers(b ?? []))
  // Re-read when a browser connects or drops, so "Connected" appears on its row.
  useEffect(refresh, [status?.browsers?.join('|')])

  useEffect(() => {
    void call<HeliumStatus>('cmd_helium_status').then(setStatus)
    let off: (() => void) | undefined
    void on<HeliumStatus>('helium:status', setStatus).then((fn) => (off = fn))
    return () => off?.()
  }, [])

  const copy = () => {
    if (!status?.extensionPath) return
    void navigator.clipboard.writeText(status.extensionPath).then(() => {
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    })
  }

  return (
    <Stack sx={{ gap: 2.5, maxWidth: 560 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          Your browser
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
          With the Vivianne extension, Vivianne can use the browser you're signed in to — Chrome, Edge, Brave, Arc, Opera,
          Vivaldi, Helium or any other Chromium browser — in tabs it groups as “Vivianne”. Install it in more than one and
          Vivianne uses the one you opened last. It asks before acting, like any other action with side effects.
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, p: 1.5, border: 1, borderColor: 'divider', borderRadius: '12px' }}>
        <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: status?.connected ? 'success.main' : 'text.disabled', flexShrink: 0 }} />
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          {status?.connected ? 'Connected' : 'Not connected'}
        </Typography>
        <Typography variant="body2" color="text.secondary" noWrap sx={{ minWidth: 0 }}>
          {status?.connected
            ? `· ${(status.browsers?.length ? status.browsers : [status.browser ?? 'browser']).join(', ')}`
            : '· open your browser with the extension installed'}
        </Typography>
      </Stack>

      {browsers.length > 0 && (
        <Stack sx={{ gap: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Browsers on this computer
          </Typography>
          <Stack sx={{ border: 1, borderColor: 'divider', borderRadius: '12px', overflow: 'hidden' }}>
            {browsers.map((b, i) => (
              <Stack
                key={b.id}
                direction="row"
                sx={{ alignItems: 'center', gap: 1.25, px: 1.5, py: 1, borderTop: i ? 1 : 0, borderColor: 'divider' }}
              >
                <Box sx={{ width: 8, height: 8, borderRadius: '50%', flexShrink: 0, bgcolor: b.connected ? 'success.main' : 'text.disabled' }} />
                <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }} noWrap>
                  {b.name}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
                  {b.connected ? 'Connected' : 'Extension not installed'}
                </Typography>
                {!b.connected && (
                  <Button
                    size="small"
                    variant="outlined"
                    disabled={opening === b.id}
                    onClick={() => {
                      setOpening(b.id)
                      setError(null)
                      call('cmd_open_browser_extensions', { id: b.id })
                        .catch((e) => setError(e instanceof Error ? e.message : String(e)))
                        .finally(() => setOpening(null))
                    }}
                    sx={{ borderRadius: '8px', flexShrink: 0 }}
                  >
                    Set up
                  </Button>
                )}
              </Stack>
            ))}
          </Stack>
          {error && (
            <Typography variant="caption" color="error.main">
              {error}
            </Typography>
          )}
          <Typography variant="caption" color="text.secondary">
            “Set up” opens that browser at its extensions page. Then follow the steps below.
          </Typography>
        </Stack>
      )}

      <Stack sx={{ gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Install the extension
        </Typography>
        <Typography component="ol" variant="body2" color="text.secondary" sx={{ m: 0, pl: 2.5, lineHeight: 1.7 }}>
          <li>
            Open your browser's extensions page: <code>chrome://extensions</code>, <code>edge://extensions</code>,{' '}
            <code>brave://extensions</code>, <code>opera://extensions</code> or <code>helium://extensions</code>.
          </li>
          <li>Turn on Developer mode, then choose Load unpacked.</li>
          <li>Pick this folder. Vivianne keeps it up to date, so you only do this once.</li>
        </Typography>
        <Box
          sx={{
            px: 1.25,
            py: 1,
            borderRadius: '8px',
            bgcolor: 'action.hover',
            fontFamily: '"Geist Mono", ui-monospace, monospace',
            fontSize: '0.75rem',
            wordBreak: 'break-all',
          }}
        >
          {status?.extensionPath ?? '…'}
        </Box>
        <Stack direction="row" sx={{ gap: 1 }}>
          <Button size="small" variant="outlined" onClick={() => void call('cmd_helium_open_extension_folder')} sx={{ borderRadius: '8px' }}>
            Open folder
          </Button>
          <Button size="small" onClick={copy} disabled={!status?.extensionPath} sx={{ borderRadius: '8px' }}>
            {copied ? 'Copied' : 'Copy path'}
          </Button>
        </Stack>
      </Stack>
    </Stack>
  )
}
