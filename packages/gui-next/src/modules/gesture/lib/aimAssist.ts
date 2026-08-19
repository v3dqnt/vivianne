/**
 * "Aim assist" for the gesture cursor — snaps the displayed pointer onto the
 * nearest clickable element within a radius, the way a game's aim assist
 * snaps a reticle onto a target. Landmark tracking alone isn't precise
 * enough to hit small UI targets (buttons, tabs, list rows) reliably, so the
 * cursor locks onto candidates instead of requiring pixel-perfect aim.
 */

export interface AimTarget {
  el: HTMLElement
  rect: DOMRect
}

const INTERACTIVE_SELECTOR = [
  'button',
  'a[href]',
  'input:not([type="hidden"])',
  'select',
  'textarea',
  '[role="button"]',
  '[role="tab"]',
  '[role="menuitem"]',
  '[role="option"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[tabindex]:not([tabindex="-1"])',
  '.MuiButtonBase-root',
].join(', ')

/** Scans the document for clickable elements currently in the viewport. Not
 * cheap enough to call every frame — callers should poll this on an
 * interval, not per pointer update. */
export function queryInteractiveTargets(): AimTarget[] {
  const nodes = document.querySelectorAll<HTMLElement>(INTERACTIVE_SELECTOR)
  const targets: AimTarget[] = []
  for (const el of nodes) {
    if (el.hasAttribute('disabled') || el.getAttribute('aria-disabled') === 'true') continue
    if (el.offsetParent === null && el.style.position !== 'fixed') continue
    const rect = el.getBoundingClientRect()
    if (rect.width === 0 || rect.height === 0) continue
    if (rect.bottom < 0 || rect.right < 0 || rect.top > window.innerHeight || rect.left > window.innerWidth) continue
    targets.push({ el, rect })
  }
  return targets
}

/** Distance from a point to a rect's nearest edge; 0 when the point is inside. */
function distanceToRect(x: number, y: number, rect: DOMRect): number {
  const dx = Math.max(rect.left - x, 0, x - rect.right)
  const dy = Math.max(rect.top - y, 0, y - rect.bottom)
  return Math.hypot(dx, dy)
}

/** Nearest target within `maxDistance`, preferring smaller (more specific)
 * elements when two candidates are roughly tied — otherwise a big container
 * div-turned-button would always win over a small icon button inside it. */
export function findNearestTarget(x: number, y: number, targets: AimTarget[], maxDistance: number): AimTarget | null {
  let best: AimTarget | null = null
  let bestDist = maxDistance
  let bestArea = Infinity
  for (const t of targets) {
    const d = distanceToRect(x, y, t.rect)
    if (d > bestDist) continue
    const area = t.rect.width * t.rect.height
    if (d < bestDist - 1 || (d <= bestDist + 1 && area < bestArea)) {
      best = t
      bestDist = d
      bestArea = area
    }
  }
  return best
}
