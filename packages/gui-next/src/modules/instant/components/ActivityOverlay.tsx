import { useEffect, useState } from 'react'
import { call, on } from '../../../ipc'
import { useTransparentWindowBody } from '../../../lib/transparentWindow'
import { Notch, type NotchTone } from './Notch'

/** Mirrors `activity::ActivityPaint`. */
interface ActivityPaint {
  state: 'working' | 'done' | 'stopped' | 'error'
  action: string
  startedAt: number
}

/**
 * "Vivianne is using your computer": the whole UI of the `viv_activity`
 * window. What she's doing now, for how long, and Stop — which also cuts off
 * typing mid-word. Shown only while the main agent drives the desktop.
 */
export function ActivityOverlay() {
  useTransparentWindowBody()
  const [paint, setPaint] = useState<ActivityPaint | null>(null)
  const [stopping, setStopping] = useState(false)

  useEffect(() => {
    let off: (() => void) | undefined
    void on<ActivityPaint>('activity:paint', (p) => {
      setPaint(p)
      if (p.state !== 'working') setStopping(false)
    }).then((fn) => {
      off = fn
      void call('cmd_overlay_ready')
    })
    return () => off?.()
  }, [])

  if (!paint) return null
  const working = paint.state === 'working'
  const tone: NotchTone = stopping ? 'stopped' : paint.state

  return (
    <Notch
      tone={tone}
      title="Vivianne"
      detail={stopping ? 'Stopping…' : paint.action}
      since={working ? paint.startedAt : null}
      onStop={
        working && !stopping
          ? () => {
              setStopping(true)
              void call('cmd_activity_stop').catch(() => setStopping(false))
            }
          : null
      }
      resizeCommand="cmd_activity_set_size"
      minWidth={200}
    />
  )
}
