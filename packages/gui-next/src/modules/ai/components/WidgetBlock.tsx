import { useContext, useEffect, useRef, useState } from 'react'
import { Alert, Box, ButtonBase, Dialog, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { CopyLinear, TickCircleLinear, MaximizeLinear, CloseLinear } from '../../../icons'
import { call } from '../../../ipc'
import { isWindows } from '../../../lib/platform'
import { useModTheme } from '../../mods/host/ModFrame'
import { useAgentStore } from '../store/agentStore'
import { FollowUpContext } from './InteractiveBlock'

/**
 * A ```widget fence: a small HTML/JS app the model wrote (a game, a
 * simulation, a tool), running live in the reply. It runs in the same
 * sandbox as mods: an opaque-origin frame with no network and no access to
 * the app, which it can only ask to size itself or send a follow-up.
 */
export function WidgetBlock({ code }: { code: string }) {
  const theme = useTheme()
  const [id, setId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [full, setFull] = useState(false)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    // While the reply streams the code grows every token: start the app
    // once it has stopped changing, not on every keystroke of the model.
    let stale = false
    const t = window.setTimeout(() => {
      call<string>('cmd_widget_put', { html: code })
        .then((v) => !stale && setId(v ?? null))
        .catch((e) => !stale && setError(e instanceof Error ? e.message : String(e)))
    }, 500)
    return () => {
      stale = true
      window.clearTimeout(t)
    }
  }, [code])

  if (error) return <Alert severity="warning" sx={{ my: 1, fontSize: '0.8125rem' }}>This widget couldn't start: {error}</Alert>
  if (!id) return <Box sx={{ my: 1.5, height: 120, borderRadius: '16px', bgcolor: alpha(theme.palette.text.primary, 0.03) }} />

  const copy = () =>
    void navigator.clipboard.writeText(code).then(() => {
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1200)
    })

  const tool = { width: 26, height: 26, borderRadius: '7px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } } as const
  return (
    <Box sx={{ my: 1.5, borderRadius: '16px', overflow: 'hidden', border: 1, borderColor: alpha(theme.palette.primary.main, 0.18), bgcolor: alpha(theme.palette.text.primary, 0.02) }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1.25, py: 0.5, borderBottom: 1, borderColor: 'divider' }}>
        <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', flex: 1 }}>Interactive</Typography>
        <Tooltip title={copied ? 'Copied' : 'Copy source'}>
          <ButtonBase aria-label="Copy source" onClick={copy} sx={tool}>
            {copied ? <TickCircleLinear size={13} color="currentColor" /> : <CopyLinear size={13} color="currentColor" />}
          </ButtonBase>
        </Tooltip>
        <Tooltip title="Open full size">
          <ButtonBase aria-label="Open full size" onClick={() => setFull(true)} sx={tool}>
            <MaximizeLinear size={13} color="currentColor" />
          </ButtonBase>
        </Tooltip>
      </Stack>
      <WidgetFrame id={id} />
      <Dialog open={full} onClose={() => setFull(false)} fullScreen>
        <Stack sx={{ height: '100%' }}>
          <Stack direction="row" sx={{ justifyContent: 'flex-end', p: 1 }}>
            <ButtonBase aria-label="Close" onClick={() => setFull(false)} sx={tool}>
              <CloseLinear size={15} color="currentColor" />
            </ButtonBase>
          </Stack>
          <Box sx={{ flex: 1, minHeight: 0 }}>
            <WidgetFrame id={id} fill />
          </Box>
        </Stack>
      </Dialog>
    </Box>
  )
}

function WidgetFrame({ id, fill = false }: { id: string; fill?: boolean }) {
  const ref = useRef<HTMLIFrameElement | null>(null)
  const [height, setHeight] = useState(240)
  const modTheme = useModTheme()
  const followUp = useContext(FollowUpContext)
  const sendMain = useAgentStore((s) => s.sendMessage)
  const src = isWindows ? `http://vivmod.localhost/_widget/${id}` : `vivmod://localhost/_widget/${id}`

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      // Only this frame's messages, and only these three.
      if (e.source !== ref.current?.contentWindow) return
      const d = e.data as { viv?: string; height?: number; text?: string }
      if (d?.viv === 'resize' && typeof d.height === 'number') setHeight(Math.min(900, Math.max(80, d.height)))
      else if (d?.viv === 'followUp' && typeof d.text === 'string' && d.text.trim()) (followUp ?? ((t: string) => void sendMain(t, [])))(d.text.slice(0, 4000))
      else if (d?.viv === 'ready') ref.current?.contentWindow?.postMessage({ viv: 'theme', ...modTheme }, '*')
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [followUp, sendMain, modTheme])

  // A theme change reaches a running widget too.
  useEffect(() => {
    ref.current?.contentWindow?.postMessage({ viv: 'theme', ...modTheme }, '*')
  }, [modTheme])

  return (
    <Box
      component="iframe"
      ref={ref}
      src={src}
      title="Interactive answer"
      sandbox="allow-scripts"
      onLoad={() => ref.current?.contentWindow?.postMessage({ viv: 'theme', ...modTheme }, '*')}
      sx={{ display: 'block', border: 0, width: '100%', height: fill ? '100%' : height, bgcolor: 'transparent' }}
    />
  )
}
