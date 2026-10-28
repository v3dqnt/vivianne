import { useEffect } from 'react'
import { Box, Stack, Typography, IconButton, Tooltip, ButtonBase } from '../../../ui'
import { AddLinear, CloseLinear, DocumentLinear, GlobalLinear } from '../../../icons'
import { usePanelStore } from '../../dock/store/panelStore'
import { useBrowserStore } from './browserStore'
import { FileTab } from './FileTab'
import { WebTab, WebToolbar, type WebTabModel } from './WebTab'
import { basename } from '../files/fileTree'

/**
 * The Browser panel: web tabs, each its own child webview, plus local files
 * opened as tabs in the same strip.
 */
export function BrowserPanel() {
  const tabs = useBrowserStore((s) => s.tabs)
  const activeId = useBrowserStore((s) => s.activeId)
  const newTab = useBrowserStore((s) => s.newTab)
  const close = useBrowserStore((s) => s.close)
  const activate = useBrowserStore((s) => s.activate)
  const openUrl = useBrowserStore((s) => s.openUrl)
  const openFile = useBrowserStore((s) => s.openFile)

  // What the rest of the app asked for: a link from the chat, a document from
  // the Files panel, the agent's web_preview.
  useEffect(() => {
    const take = () => {
      const p = usePanelStore.getState().takePending()
      if (!p) return
      if (p.kind === 'url') openUrl(p.url)
      else openFile(p.path, basename(p.path))
    }
    take()
    return usePanelStore.subscribe(take)
  }, [openUrl, openFile])

  const active = tabs.find((t) => t.id === activeId) ?? null

  return (
    <Stack sx={{ flex: 1, minHeight: 0, minWidth: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, px: 1, pt: 0.75, flexShrink: 0, overflowX: 'auto' }}>
        {tabs.map((tab) => {
          const on = tab.id === activeId
          return (
            <Stack
              key={tab.id}
              direction="row"
              onClick={() => activate(tab.id)}
              sx={{
                alignItems: 'center', gap: 0.5, height: 26, pl: 1, pr: 0.5, flexShrink: 0, maxWidth: 180,
                borderRadius: '7px', cursor: 'pointer',
                bgcolor: on ? 'action.selected' : 'transparent',
                '&:hover': { bgcolor: on ? 'action.selected' : 'action.hover' },
                '&:hover .tab-close': { opacity: 1 },
              }}
              title={tab.kind === 'web' ? tab.url : tab.path}
            >
              {tab.kind === 'web' ? <GlobalLinear size={11} color="currentColor" /> : <DocumentLinear size={11} color="currentColor" />}
              <Typography variant="caption" noWrap sx={{ flex: 1, minWidth: 0, fontSize: '0.6875rem', color: on ? 'text.primary' : 'text.secondary' }}>
                {tab.title}
              </Typography>
              <Box
                component="button"
                type="button"
                className="tab-close"
                aria-label={`Close ${tab.title}`}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation()
                  close(tab.id)
                }}
                sx={{
                  border: 0, bgcolor: 'transparent', color: 'inherit', p: 0.25, borderRadius: '4px', cursor: 'pointer',
                  display: 'grid', opacity: on ? 0.6 : 0, transition: 'opacity 0.12s',
                  '&:hover': { bgcolor: 'action.hover', opacity: 1 },
                }}
              >
                <CloseLinear size={10} color="currentColor" />
              </Box>
            </Stack>
          )
        })}
        <Tooltip title="New tab">
          <IconButton size="small" aria-label="New tab" onClick={newTab} sx={{ width: 24, height: 24, flexShrink: 0 }}>
            <AddLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>

      {active === null ? (
        <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 1 }}>
          <Typography variant="caption" color="text.secondary">No tabs open.</Typography>
          <ButtonBase
            onClick={newTab}
            sx={{ px: 1.5, height: 28, borderRadius: '8px', border: 1, borderColor: 'divider', fontSize: '0.75rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
          >
            New tab
          </ButtonBase>
        </Stack>
      ) : (
        <>
        {active.kind === 'web' && <WebToolbar key={active.id} tab={active as WebTabModel} />}
        <Box sx={{ position: 'relative', flex: 1, minHeight: 0, bgcolor: 'background.default' }}>
          {/* Every web tab stays mounted: its webview is an OS-level view that
              only this component can park. Inactive ones are off-screen. */}
          {tabs.filter((t) => t.kind === 'web').map((t) => (
            <WebTab key={t.id} tab={t as WebTabModel} active={t.id === activeId} />
          ))}
          {active.kind === 'file' && <Box sx={{ position: 'absolute', inset: 0 }}><FileTab path={active.path} /></Box>}
        </Box>
        </>
      )}
    </Stack>
  )
}
