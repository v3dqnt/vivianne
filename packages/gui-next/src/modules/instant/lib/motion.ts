import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import gsap from 'gsap'

/** The notch's one ease: quick out, soft landing. Matches the CSS `EASE`. */
export const EASE_OUT = 'expo.out'

/** Users who asked for less motion get the end state, instantly. */
export const reducedMotion = () => window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false

/**
 * Fade-and-drop the `[data-anim]` children of `ref` into place, one after the
 * other, whenever `key` changes. Content stays visible if GSAP never runs.
 */
export function useStaggerIn(ref: RefObject<HTMLElement | null>, key: unknown) {
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is the trigger, not an input
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || reducedMotion()) return
    const ctx = gsap.context(() => {
      gsap.from('[data-anim]', { y: -6, opacity: 0, duration: 0.42, ease: EASE_OUT, stagger: 0.035, clearProps: 'all' })
    }, el)
    return () => ctx.revert()
  }, [ref, key])
}

/** A small springy pop on `ref` whenever `key` changes (not on mount). */
export function usePop(ref: RefObject<HTMLElement | null>, key: unknown) {
  const first = useRef(true)
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is the trigger, not an input
  useEffect(() => {
    if (first.current) {
      first.current = false
      return
    }
    const el = ref.current
    if (!el || reducedMotion()) return
    const t = gsap.fromTo(el, { scale: 0.82 }, { scale: 1, duration: 0.5, ease: 'back.out(2.6)', clearProps: 'transform' })
    return () => void t.kill()
  }, [ref, key])
}

/** `value`, counted up from where it last was rather than snapping. */
export function useCountUp(value: number, duration = 0.7): number {
  const [shown, setShown] = useState(value)
  const from = useRef(value)
  useEffect(() => {
    if (reducedMotion()) {
      from.current = value
      setShown(value)
      return
    }
    const box = { v: from.current }
    const t = gsap.to(box, {
      v: value,
      duration,
      ease: 'power3.out',
      onUpdate: () => setShown(box.v),
      onComplete: () => {
        from.current = value
      },
    })
    return () => {
      from.current = box.v
      t.kill()
    }
  }, [value, duration])
  return shown
}
