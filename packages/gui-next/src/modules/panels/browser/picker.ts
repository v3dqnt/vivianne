/** What the injected script leaves on the page when an element is picked. */
export interface PickResult {
  selector: string
  tag: string
  text: string
  rect: { x: number; y: number; width: number; height: number }
  url: string
  title: string
  styles?: Record<string, string>
}

/**
 * Installs the picker in the page: a crosshair cursor, an outline that follows
 * the hovered element, and capture-phase listeners that swallow the click and
 * record the element instead of following the link.
 *
 * Runs as one self-contained script because a child webview on an external URL
 * gets no IPC surface — this is the only way in.
 */
export const PICK_JS = `(() => {
  if (window.__vivPickMode) return 'already'
  window.__vivPickMode = true
  const style = document.createElement('style')
  style.id = '__vivPickStyle'
  style.textContent = '.__vivPickBox{position:fixed;pointer-events:none;z-index:2147483646;border:2px solid #4c8dff;background:rgba(76,141,255,0.12);border-radius:2px}'
    + '.__vivPickTag{position:fixed;z-index:2147483647;pointer-events:none;background:#4c8dff;color:#fff;font:600 11px/1.4 ui-monospace,monospace;padding:2px 6px;border-radius:3px;white-space:nowrap}'
  document.documentElement.appendChild(style)
  document.documentElement.style.cursor = 'crosshair'
  const box = document.createElement('div')
  box.className = '__vivPickBox'
  const tag = document.createElement('div')
  tag.className = '__vivPickTag'
  document.documentElement.appendChild(box)
  document.documentElement.appendChild(tag)

  const pathOf = (el) => {
    if (el.id) return '#' + el.id
    const testid = el.getAttribute && el.getAttribute('data-testid')
    if (testid) return '[data-testid="' + testid + '"]'
    const parts = []
    let node = el
    while (node && node.nodeType === 1 && node !== document.documentElement) {
      let part = node.tagName.toLowerCase()
      if (node.classList && node.classList.length) part += '.' + Array.from(node.classList).slice(0, 2).join('.')
      const parent = node.parentElement
      if (parent) {
        const same = Array.from(parent.children).filter((c) => c.tagName === node.tagName)
        if (same.length > 1) part += ':nth-of-type(' + (same.indexOf(node) + 1) + ')'
      }
      parts.unshift(part)
      node = parent
    }
    return parts.join(' > ')
  }

  const styleOf = (el) => {
    const s = getComputedStyle(el)
    return {
      fontFamily: s.fontFamily,
      fontSize: s.fontSize,
      fontWeight: s.fontWeight,
      color: s.color,
      background: s.backgroundColor,
      padding: s.padding,
      margin: s.margin,
      borderRadius: s.borderRadius,
      display: s.display,
    }
  }

  const over = (e) => {
    const el = e.target
    if (!el || el.nodeType !== 1) return
    const r = el.getBoundingClientRect()
    box.style.left = r.left + 'px'
    box.style.top = r.top + 'px'
    box.style.width = r.width + 'px'
    box.style.height = r.height + 'px'
    tag.textContent = pathOf(el)
    tag.style.left = Math.max(0, r.left) + 'px'
    tag.style.top = Math.max(0, r.top - 18) + 'px'
  }

  const click = (e) => {
    e.preventDefault()
    e.stopPropagation()
    const el = e.target
    const r = el.getBoundingClientRect()
    window.__vivPick = {
      selector: pathOf(el),
      tag: el.tagName.toLowerCase(),
      text: (el.innerText || el.textContent || '').trim().slice(0, 120),
      rect: { x: r.left, y: r.top, width: r.width, height: r.height },
      url: location.href,
      title: document.title,
      styles: styleOf(el),
    }
    cleanup()
  }

  const key = (e) => {
    if (e.key === 'Escape') {
      window.__vivPick = null
      cleanup()
    }
  }

  const cleanup = () => {
    window.__vivPickMode = false
    window.__vivPickStop = null
    document.documentElement.style.cursor = ''
    box.remove()
    tag.remove()
    style.remove()
    document.removeEventListener('mouseover', over, true)
    document.removeEventListener('click', click, true)
    document.removeEventListener('keydown', key, true)
  }

  window.__vivPickStop = cleanup
  document.addEventListener('mouseover', over, true)
  document.addEventListener('click', click, true)
  document.addEventListener('keydown', key, true)
  return 'picking'
})()`

/** Read and forget what was picked. */
export const PICK_READ_JS = '(() => { const p = window.__vivPick || null; window.__vivPick = null; return p })()'

/** Take the overlay down without picking anything. */
export const PICK_CANCEL_JS = '(() => { if (window.__vivPickStop) window.__vivPickStop(); return true })()'

/** The draft a pick turns into, appended to the chat composer. */
export function commentDraft(pick: PickResult, comment: string, screenshot?: string | null): string {
  const label = pick.text ? `${pick.selector} "${pick.text}"` : pick.selector
  const lines = [`Comment on ${label} — ${pick.url}`, comment.trim()]
  if (screenshot) lines.push(`Screenshot: ${screenshot}`)
  return lines.join('\n')
}
