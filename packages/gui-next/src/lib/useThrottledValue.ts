import { useEffect, useRef, useState } from 'react'

/** Leading+trailing throttle: the first value applies immediately (no
 * startup lag for already-settled content), further changes within
 * `intervalMs` coalesce, and the latest value always lands eventually so a
 * stream that goes quiet doesn't get stuck showing stale text. */
export function useThrottledValue<T>(value: T, intervalMs: number): T {
  const [throttled, setThrottled] = useState(value)
  const lastRunAt = useRef(0)
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null)
  const latest = useRef(value)
  latest.current = value

  useEffect(() => {
    const elapsed = Date.now() - lastRunAt.current
    if (elapsed >= intervalMs) {
      lastRunAt.current = Date.now()
      setThrottled(value)
    } else if (!pending.current) {
      pending.current = setTimeout(() => {
        pending.current = null
        lastRunAt.current = Date.now()
        setThrottled(latest.current)
      }, intervalMs - elapsed)
    }
    return () => {
      if (pending.current) {
        clearTimeout(pending.current)
        pending.current = null
      }
    }
  }, [value, intervalMs])

  return throttled
}
