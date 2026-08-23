import { useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import InputBase from '@mui/material/InputBase'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import CircularProgress from '@mui/material/CircularProgress'
import { ArrowLeftLinear, ArrowRightLinear, RefreshLinear, GlobalLinear } from '../../../icons'
import { useWebPreviewStore } from '../../ai/store/webPreviewStore'

/** General-purpose embedded browser: two sources feed the same view.
 *
 * - **Agent-driven**: when the agent is actively controlling camoufox-browser
 *   (via web_preview/browser_click/browser_type/etc — see `webPreviewStore`),
 *   this shows the real screenshot it's producing, with back/forward/refresh/
 *   navigate wired to the same camoufox session the agent is using. This is
 *   what makes the merged Preview panel "intelligent": it's the one place
 *   agent-driven browsing is visible (replaces the old standalone
 *   WebPreviewPanel dialog).
 * - **Manual fallback**: otherwise, a plain address bar + iframe.
 *   ponytail: scoped to local URLs (CSP's `frame-src` only allows
 *   localhost/127.0.0.1, same as the dev-server mode) — add `https:` to
 *   frame-src in tauri.conf.json if browsing arbitrary external sites
 *   through the manual iframe is wanted; agent-driven browsing already isn't
 *   subject to this since camoufox is a real, separate browser process. */
export function BrowserPanel() {
  const agentActive = useWebPreviewStore((s) => s.open)
  const agentUrl = useWebPreviewStore((s) => s.url)
  const agentScreenshot = useWebPreviewStore((s) => s.screenshotBase64)
  const agentError = useWebPreviewStore((s) => s.error)
  const agentLoading = useWebPreviewStore((s) => s.loading)
  const agentBack = useWebPreviewStore((s) => s.back)
  const agentForward = useWebPreviewStore((s) => s.forward)
  const agentRefresh = useWebPreviewStore((s) => s.refresh)
  const agentNavigate = useWebPreviewStore((s) => s.navigate)

  const [input, setInput] = useState('http://localhost:1420')
  const [url, setUrl] = useState(input)
  const [reloadKey, setReloadKey] = useState(0)
  const [addressDraft, setAddressDraft] = useState('')

  if (agentActive) {
    return (
      <Stack sx={{ flex: 1, minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, p: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
          <IconButton size="small" onClick={() => void agentBack()} disabled={agentLoading}>
            <ArrowLeftLinear size={14} color="currentColor" />
          </IconButton>
          <IconButton size="small" onClick={() => void agentForward()} disabled={agentLoading}>
            <ArrowRightLinear size={14} color="currentColor" />
          </IconButton>
          <IconButton size="small" onClick={() => void agentRefresh()} disabled={agentLoading}>
            {agentLoading ? <CircularProgress size={12} /> : <RefreshLinear size={13} color="currentColor" />}
          </IconButton>
          <InputBase
            fullWidth
            value={addressDraft || agentUrl}
            onChange={(e) => setAddressDraft(e.target.value)}
            onFocus={() => setAddressDraft(agentUrl)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && addressDraft.trim()) void agentNavigate(addressDraft.trim())
            }}
            sx={{ fontSize: '0.8125rem', fontFamily: '"JetBrains Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.5 }}
          />
        </Stack>
        <Box sx={{ flex: 1, minHeight: 0, display: 'flex', bgcolor: 'action.hover' }}>
          {agentError ? (
            <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', p: 3 }}>
              <Typography variant="body2" color="error.main">
                {agentError}
              </Typography>
            </Stack>
          ) : agentScreenshot ? (
            <Box
              component="img"
              src={`data:image/png;base64,${agentScreenshot}`}
              sx={{ width: '100%', flex: 1, objectFit: 'contain', bgcolor: 'background.default' }}
            />
          ) : (
            <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
              <CircularProgress size={20} />
            </Stack>
          )}
        </Box>
      </Stack>
    )
  }

  const go = () => {
    const next = /^https?:\/\//.test(input) ? input : `http://${input}`
    setInput(next)
    setUrl(next)
    setReloadKey((k) => k + 1)
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, p: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <GlobalLinear size={13} color="currentColor" />
        <InputBase
          fullWidth
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && go()}
          sx={{ fontSize: '0.8125rem', fontFamily: '"JetBrains Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.5 }}
        />
        <IconButton size="small" onClick={() => setReloadKey((k) => k + 1)}>
          <RefreshLinear size={14} color="currentColor" />
        </IconButton>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, position: 'relative' }}>
        <iframe
          key={reloadKey}
          src={url}
          title="Browser"
          style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', border: 'none', display: 'block' }}
        />
      </Box>
    </Stack>
  )
}
