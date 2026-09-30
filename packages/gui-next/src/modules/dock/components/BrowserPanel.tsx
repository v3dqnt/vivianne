import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, InputBase, IconButton, Typography, CircularProgress } from '../../../ui'
import { ArrowLeftLinear, ArrowRightLinear, RefreshLinear, GlobalLinear } from '../../../icons'
import { useWebPreviewStore } from '../../ai/store/webPreviewStore'
import { useActiveWorkspaceRoot } from '../useActiveWorkspace'
import { useDevServerStore } from '../store/devServerStore'
import { devServerId } from '../lib/devServer'

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
  const clickAt = useWebPreviewStore((s) => s.clickAt)
  const typeText = useWebPreviewStore((s) => s.typeText)
  const pressKey = useWebPreviewStore((s) => s.pressKey)
  const scrollBy = useWebPreviewStore((s) => s.scrollBy)
  const startLivePolling = useWebPreviewStore((s) => s.startLivePolling)
  const stopLivePolling = useWebPreviewStore((s) => s.stopLivePolling)

  // Keep the agent-driven view live. Without this the panel only updated
  // when a browser tool happened to fire, so it froze between calls — during
  // a page load, a redirect, or a long model turn — which looks like the
  // browser has hung rather than like it is working.
  useEffect(() => {
    if (!agentActive) return
    startLivePolling()
    return () => stopLivePolling()
  }, [agentActive, startLivePolling, stopLivePolling])

  // The manual address bar used to default to http://localhost:1420 and load
  // it immediately — which is Vivianne's OWN dev server, so opening the
  // Browser tab rendered the app inside itself instead of anything the user
  // was working on. Default to the workspace's running dev server when there
  // is one, and otherwise start empty rather than guessing a port.
  const root = useActiveWorkspaceRoot()
  const servers = useDevServerStore((s) => s.servers)
  // LivePreviewPanel refreshes on mount, but the two modes are siblings —
  // opening straight into Browser mode would otherwise see an empty list and
  // offer no shortcut to a dev server that is already running.
  const refreshServers = useDevServerStore((s) => s.refresh)
  useEffect(() => {
    void refreshServers()
  }, [refreshServers])
  const workspaceUrl = useMemo(() => {
    if (!root) return ''
    const id = devServerId(root)
    const status = servers.find(([c]) => c.id === id)?.[1]
    return status?.kind === 'running' ? status.url : ''
  }, [servers, root])

  const [input, setInput] = useState('')
  // Nothing is loaded until the user asks for it — an iframe pointed at a
  // guessed address is either a dead frame or, worse, this app again.
  const [url, setUrl] = useState('')
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
            sx={{ fontSize: '0.8125rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.5 }}
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
            // The screenshot is an input surface, not just a picture. The agent
            // acts on elements by accessibility ref; a person has only a pointer,
            // so their clicks are sent as a FRACTION of the rendered image.
            // `objectFit: contain` letterboxes the image inside this box, so the
            // box's own rect is not the image's rect — measuring the element and
            // dividing would be wrong by the letterbox margin on one axis. The
            // real drawn rect is derived from naturalWidth/Height against the
            // element size below.
            <Box
              component="img"
              tabIndex={0}
              src={`data:image/png;base64,${agentScreenshot}`}
              onClick={(e: React.MouseEvent<HTMLImageElement>) => {
                const img = e.currentTarget
                const box = img.getBoundingClientRect()
                if (!img.naturalWidth || !img.naturalHeight) return
                const scale = Math.min(box.width / img.naturalWidth, box.height / img.naturalHeight)
                const drawnW = img.naturalWidth * scale
                const drawnH = img.naturalHeight * scale
                const offsetX = (box.width - drawnW) / 2
                const offsetY = (box.height - drawnH) / 2
                const fx = (e.clientX - box.left - offsetX) / drawnW
                const fy = (e.clientY - box.top - offsetY) / drawnH
                // A click in the letterbox margin isn't on the page at all.
                if (fx < 0 || fx > 1 || fy < 0 || fy > 1) return
                img.focus()
                void clickAt(fx, fy)
              }}
              onWheel={(e: React.WheelEvent<HTMLImageElement>) => {
                void scrollBy(e.deltaY > 0 ? 'down' : 'up', Math.min(800, Math.abs(e.deltaY) * 3))
              }}
              onKeyDown={(e: React.KeyboardEvent<HTMLImageElement>) => {
                // Only meaningful after a click has focused something in the
                // page; typing into a page with no focused field is a no-op
                // there, same as it would be in a real browser.
                if (e.metaKey || e.ctrlKey || e.altKey) return
                if (e.key.length === 1) {
                  e.preventDefault()
                  void typeText(e.key)
                } else if (['Enter', 'Tab', 'Backspace', 'Delete', 'Escape', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) {
                  e.preventDefault()
                  void pressKey(e.key)
                }
              }}
              sx={{
                width: '100%',
                flex: 1,
                objectFit: 'contain',
                bgcolor: 'background.default',
                cursor: 'pointer',
                outline: 'none',
                '&:focus-visible': { boxShadow: (t) => `inset 0 0 0 2px ${t.palette.primary.main}` },
              }}
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

  const go = (raw?: string) => {
    const target = (raw ?? input).trim()
    if (!target) return
    const next = /^https?:\/\//.test(target) ? target : `http://${target}`
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
          placeholder={workspaceUrl || 'Enter a local address, e.g. localhost:3000'}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && go(input || workspaceUrl)}
          sx={{ fontSize: '0.8125rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.5 }}
        />
        <IconButton size="small" onClick={() => setReloadKey((k) => k + 1)}>
          <RefreshLinear size={14} color="currentColor" />
        </IconButton>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, position: 'relative', display: 'flex' }}>
        {url ? (
          <iframe
            key={reloadKey}
            src={url}
            title="Browser"
            style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', border: 'none', display: 'block' }}
          />
        ) : (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 1, p: 3 }}>
            <GlobalLinear size={22} color="currentColor" />
            <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
              {workspaceUrl
                ? 'Enter an address, or open this workspace’s dev server.'
                : 'Enter a local address to load it here.'}
            </Typography>
            {workspaceUrl && (
              <Typography
                variant="caption"
                onClick={() => go(workspaceUrl)}
                sx={{ cursor: 'pointer', color: 'primary.main', fontFamily: '"Geist Mono", ui-monospace, monospace' }}
              >
                {workspaceUrl}
              </Typography>
            )}
          </Stack>
        )}
      </Box>
    </Stack>
  )
}
