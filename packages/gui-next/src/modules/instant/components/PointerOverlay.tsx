import { useEffect, useState } from 'react'
import { Box, Typography, alpha, keyframes, useTheme } from '../../../ui'
import { call, on } from '../../../ipc'
import { useTransparentWindowBody } from '../../../lib/transparentWindow'

/** Mirrors `instant::pointer::PointerPaint`. */
interface PointerPaint {
  kind: 'point' | 'highlight'
  width: number
  height: number
  label: string | null
}

const breathe = keyframes`
  0%   { transform: scale(1);    opacity: 0.95; }
  50%  { transform: scale(1.14); opacity: 0.65; }
  100% { transform: scale(1);    opacity: 0.95; }
`

/**
 * Viv Instant's narration cursor — the whole UI of the `viv_pointer` window.
 *
 * Draws either a pulsing ring on a point or an outline around a region, with an
 * optional caption. It is purely decorative by design: the window is
 * click-through on the Rust side, so nothing here can receive a mouse event
 * even if it wanted to.
 *
 * It animates because a static ring over a busy screen is genuinely easy to
 * miss, and the entire purpose of this window is to be noticed.
 */
export function PointerOverlay() {
  const theme = useTheme()
  useTransparentWindowBody()
  const [paint, setPaint] = useState<PointerPaint | null>(null)

  useEffect(() => {
    let off: (() => void) | undefined
    void on<PointerPaint>('pointer:paint', setPaint).then((fn) => {
      off = fn
      // Now listening: the backend delivers anything it queued while this
      // window was still loading (see `overlays.rs`).
      void call('cmd_overlay_ready')
    })
    return () => off?.()
  }, [])

  if (!paint) return null

  const accent = theme.palette.primary.main

  if (paint.kind === 'highlight') {
    return (
      <Box sx={{ position: 'relative', width: '100vw', height: '100vh' }}>
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            border: `2.5px solid ${accent}`,
            borderRadius: '10px',
            boxShadow: `0 0 0 2px ${alpha(accent, 0.25)}, 0 0 22px 4px ${alpha(accent, 0.45)}`,
            animation: `${breathe} 1.8s ease-in-out infinite`,
          }}
        />
        {paint.label && <Caption text={paint.label} accent={accent} sx={{ top: -4, left: 0 }} />}
      </Box>
    )
  }

  // The window is placed so the target is 18px in from its top-left
  // (`instant::pointer::point`); the arrow's tip sits exactly there.
  return (
    <Box sx={{ position: 'relative', width: '100vw', height: '100vh' }}>
      <Box
        sx={{
          position: 'absolute',
          top: 8,
          left: 8,
          width: 20,
          height: 20,
          borderRadius: '50%',
          bgcolor: alpha(accent, 0.35),
          animation: `${breathe} 1.3s ease-in-out infinite`,
        }}
      />
      <Box
        component="svg"
        viewBox="0 0 512 512"
        aria-hidden
        sx={{
          position: 'absolute',
          // Tip of the path is at (136, 84) of 512: at 30px that is (8, 5).
          top: 13,
          left: 10,
          width: 30,
          height: 30,
          overflow: 'visible',
          filter: 'drop-shadow(0 2px 4px rgba(0,0,0,0.45))',
        }}
      >
        <path d={ARROW} fill="#0a0a0a" stroke="#ffffff" strokeWidth={34} strokeLinejoin="round" paintOrder="stroke" />
      </Box>
      {paint.label && <Caption text={paint.label} accent={accent} sx={{ top: 4, left: 44 }} />}
    </Box>
  )
}

/** The pointer: a rounded arrow, tip at top-left. */
const ARROW =
  'M128 116 C126 74 172 52 208 80 L428 262 C458 287 446 334 404 334 L326 334 C302 334 282 346 266 367 L214 432 C190 462 146 450 144 408 Z'

function Caption({
  text,
  accent,
  sx,
}: {
  text: string
  accent: string
  sx: Record<string, unknown>
}) {
  return (
    <Typography
      variant="caption"
      sx={{
        position: 'absolute',
        px: 0.875,
        py: 0.25,
        maxWidth: 200,
        borderRadius: '8px',
        // Opaque rather than translucent: this sits over arbitrary application
        // pixels, and a caption you cannot read is worse than none.
        bgcolor: 'background.paper',
        border: 1,
        borderColor: alpha(accent, 0.5),
        color: 'text.primary',
        fontSize: '0.6875rem',
        fontWeight: 600,
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        boxShadow: '0 4px 14px rgba(0,0,0,0.4)',
        ...sx,
      }}
    >
      {text}
    </Typography>
  )
}
