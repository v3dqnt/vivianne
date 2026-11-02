// Confines a captured stylesheet selector to the captured app, so app CSS
// cannot restyle the rest of the trailer. html/:root become .viv-app, body
// becomes .viv-app .viv-body, everything else is prefixed with .viv-app.
// Also runs in the page (capture.cjs passes its source to page.evaluate).
function scopeSelector(selector) {
  const parts = []
  let depth = 0
  let start = 0
  for (let i = 0; i < selector.length; i++) {
    const c = selector[i]
    if (c === '(') depth++
    else if (c === ')') depth--
    else if (c === ',' && depth === 0) {
      parts.push(selector.slice(start, i))
      start = i + 1
    }
  }
  parts.push(selector.slice(start))
  return parts
    .map((raw) => {
      const s = raw.trim()
      if (s.startsWith('.viv-app')) return s
      if (/^(html|:root)(?=$|[\s>:.[])/.test(s)) return s.replace(/^(html|:root)/, '.viv-app')
      if (/^body(?=$|[\s>:.[])/.test(s)) return s.replace(/^body/, '.viv-app .viv-body')
      return `.viv-app ${s}`
    })
    .join(', ')
}

if (typeof module !== 'undefined') module.exports = { scopeSelector }
