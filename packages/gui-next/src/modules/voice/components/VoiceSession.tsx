import { useEffect, useRef } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import { useTheme, keyframes } from '@mui/material/styles'
import { CloseCircleLinear } from '../../../icons'
import { glassSurface } from '../../../theme'
import { on } from '../../../ipc'
import { useVoiceStore, type VoiceStatus } from '../store/voiceStore'
import { useVoicePipeline } from '../lib/useVoicePipeline'

const pulse = keyframes`
  0%, 100% { transform: scale(1); opacity: 0.85; }
  50% { transform: scale(1.15); opacity: 1; }
`

// 'idle' has no visible label — the window is hidden in that state (see `visible` below).
const STATUS_LABEL: Record<VoiceStatus, string> = {
  idle: '',
  starting: 'Starting…',
  listening: 'Listening',
  transcribing: 'Transcribing…',
  responding: 'Thinking…',
  speaking: 'Speaking',
  error: 'Voice error',
}

/** The notch window's entire UI — a small always-on-top pill that opens into
 * a taller card once there's a transcript/reply to show. Owns the window's
 * own show/hide/resize/centering via Tauri's window APIs; nothing else about
 * this window's chrome comes from `App.tsx`'s normal shell. */
export function VoiceSession() {
  const theme = useTheme()
  const GLASS = glassSurface(theme.palette.mode)
  const { openSession, closeSession, isSessionOpen } = useVoicePipeline()
  const status = useVoiceStore((s) => s.status)
  const transcript = useVoiceStore((s) => s.transcript)
  const lastReply = useVoiceStore((s) => s.lastReply)
  const errorMessage = useVoiceStore((s) => s.errorMessage)

  // Centre on the primary monitor once, on mount — tauri.conf.json's x/y is a
  // fixed guess that only happens to be right on one screen size.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const { getCurrentWindow, currentMonitor } = await import('@tauri-apps/api/window')
        const { LogicalPosition } = await import('@tauri-apps/api/dpi')
        const win = getCurrentWindow()
        const monitor = await currentMonitor()
        if (cancelled || !monitor) return
        const scale = monitor.scaleFactor || 1
        const screenW = monitor.size.width / scale
        await win.setPosition(new LogicalPosition(Math.round((screenW - 520) / 2), 12))
      } catch (e) {
        console.error('[voice] notch centering failed', e)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  // Show/hide/resize the window to match session state — hidden while idle,
  // a small pill once starting, a taller card once there's text to show.
  const visible = status !== 'idle'
  const tall = (status === 'responding' || status === 'speaking') && !!(lastReply || transcript)
  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow')
        const { LogicalSize } = await import('@tauri-apps/api/dpi')
        const win = getCurrentWebviewWindow()
        if (cancelled) return
        if (visible) {
          await win.setSize(new LogicalSize(520, tall ? 220 : 96))
          await win.show()
          await win.setAlwaysOnTop(true)
        } else {
          await win.hide()
        }
      } catch (e) {
        console.error('[voice] notch show/hide failed', e)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [visible, tall])

  // The global shortcut (Ctrl+Shift+V, registered backend-side) broadcasts
  // this event to every window. Toggles the session rather than only opening
  // it, so the same shortcut can end a conversation early. This window is
  // mounted for the app's whole lifetime (hidden), so this must stay gated
  // on the Settings toggle rather than opening on mount.
  const enabled = useVoiceStore((s) => s.enabled)
  const enabledRef = useRef(enabled)
  enabledRef.current = enabled
  useEffect(() => {
    let unlisten: (() => void) | null = null
    void on('voice-wake', () => {
      if (!enabledRef.current) return
      if (isSessionOpen()) closeSession()
      else void openSession()
    }).then((fn) => {
      unlisten = fn
    })
    return () => unlisten?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Turning the setting off mid-session ends it immediately.
  useEffect(() => {
    if (!enabled && isSessionOpen()) closeSession()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled])

  if (!visible) return null

  const showReply = (status === 'responding' || status === 'speaking') && (lastReply || transcript)

  return (
    <Box sx={{ position: 'fixed', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', pt: 0.5, bgcolor: 'transparent' }}>
      <Box
        sx={{
          ...GLASS,
          borderRadius: 999,
          display: 'flex',
          alignItems: 'center',
          gap: 1.25,
          px: 2,
          py: 1,
          maxWidth: 480,
        }}
      >
        <Box
          sx={{
            width: 10,
            height: 10,
            borderRadius: '50%',
            flexShrink: 0,
            bgcolor: status === 'error' ? 'error.main' : 'primary.main',
            animation: status === 'listening' || status === 'speaking' ? `${pulse} 1.2s ease-in-out infinite` : 'none',
          }}
        />
        <Typography variant="caption" sx={{ fontWeight: 600, flex: 1 }} noWrap>
          {status === 'error' && errorMessage ? errorMessage : STATUS_LABEL[status]}
        </Typography>
        <IconButton size="small" onClick={closeSession} title="Close voice session">
          <CloseCircleLinear size={14} color="currentColor" />
        </IconButton>
      </Box>

      {showReply && (
        <Box sx={{ ...GLASS, borderRadius: 2, mt: 1, px: 2, py: 1.5, maxWidth: 460 }}>
          {transcript && (
            <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block', mb: 0.5 }}>
              You: "{transcript}"
            </Typography>
          )}
          <Typography
            variant="body2"
            sx={{
              display: '-webkit-box',
              WebkitLineClamp: 4,
              WebkitBoxOrient: 'vertical',
              overflow: 'hidden',
            }}
          >
            {lastReply}
          </Typography>
        </Box>
      )}
    </Box>
  )
}
