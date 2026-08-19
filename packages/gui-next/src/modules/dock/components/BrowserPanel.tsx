import { useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import InputBase from '@mui/material/InputBase'
import IconButton from '@mui/material/IconButton'
import { RefreshLinear, GlobalLinear } from '../../../icons'

/** General-purpose embedded browser tab — address bar + iframe.
 * ponytail: scoped to local URLs for now (CSP's `frame-src` only allows
 * localhost/127.0.0.1, same as Live Preview) — add `https:` to frame-src in
 * tauri.conf.json if browsing arbitrary external sites is actually wanted. */
export function BrowserPanel() {
  const [input, setInput] = useState('http://localhost:1420')
  const [url, setUrl] = useState(input)
  const [reloadKey, setReloadKey] = useState(0)

  const go = () => {
    const next = /^https?:\/\//.test(input) ? input : `http://${input}`
    setInput(next)
    setUrl(next)
    setReloadKey((k) => k + 1)
  }

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
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
      <Box sx={{ flex: 1, minHeight: 0 }}>
        <iframe key={reloadKey} src={url} title="Browser" style={{ width: '100%', height: '100%', border: 'none', display: 'block' }} />
      </Box>
    </Stack>
  )
}
