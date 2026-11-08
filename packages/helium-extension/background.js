// Vivianne: the bridge between Vivianne and this browser. Works in any
// Chromium browser (Chrome, Edge, Brave, Arc, Opera, Vivaldi, Helium…): it
// uses only standard extension APIs, and its id is the same in all of them.
//
// Vivianne listens on ws://127.0.0.1:47321/helium and accepts only this
// extension (it checks the connection's Origin, which pages can't fake).
// It sends { id, method, params }; we answer { id, result } or { id, error }.
//
// Input goes through the debugger protocol (CDP), so clicks and keys are
// real, trusted events, the same as a person's. Tabs Vivianne opens are kept
// in a "Vivianne" tab group, so it's always clear which tabs it is driving.

const URL_ = 'ws://127.0.0.1:47321/helium'
let ws = null
let backoff = 1000
let lastTab = null
const attached = new Set()

function setBadge(connected) {
  chrome.action.setBadgeText({ text: connected ? '' : '!' })
  chrome.action.setBadgeBackgroundColor({ color: '#b45309' })
  chrome.storage.session.set({ connected })
}

// "Microsoft Edge 140", "Brave 140", "Google Chrome 140"…: the brand that
// isn't the shared engine or the GREASE placeholder. Browsers that report
// only "Chromium" (Arc, Vivaldi) are told apart by their user agent.
function browserName() {
  const ua = navigator.userAgent
  const known = [['OPR/', 'Opera'], ['Vivaldi/', 'Vivaldi'], ['Arc/', 'Arc'], ['YaBrowser/', 'Yandex']].find(([tag]) => ua.includes(tag))
  const brands = navigator.userAgentData?.brands ?? []
  const own = brands.find((b) => !/chromium|not.?a.?brand/i.test(b.brand))
  const version = (own ?? brands.find((b) => /chromium/i.test(b.brand)))?.version ?? ''
  if (own) return `${own.brand} ${version}`.trim()
  if (known) return `${known[1]} ${version}`.trim()
  return `Chromium ${version}`.trim()
}

function connect() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return
  try {
    ws = new WebSocket(URL_)
  } catch {
    return scheduleReconnect()
  }
  ws.onopen = () => {
    backoff = 1000
    setBadge(true)
    ws.send(JSON.stringify({ type: 'hello', version: chrome.runtime.getManifest().version, browser: browserName() }))
  }
  ws.onclose = () => {
    setBadge(false)
    scheduleReconnect()
  }
  ws.onerror = () => {}
  ws.onmessage = async (ev) => {
    let msg
    try {
      msg = JSON.parse(ev.data)
    } catch {
      return
    }
    if (msg.type === 'ping') return ws.send(JSON.stringify({ type: 'pong' }))
    if (!msg.id) return
    try {
      const result = await handle(msg.method, msg.params ?? {})
      ws.send(JSON.stringify({ id: msg.id, result }))
    } catch (e) {
      ws.send(JSON.stringify({ id: msg.id, error: String(e?.message ?? e) }))
    }
  }
}

function scheduleReconnect() {
  setTimeout(connect, backoff)
  backoff = Math.min(backoff * 2, 15000)
}

// A service worker sleeps when idle; the alarm wakes it to reconnect, and an
// open socket with traffic keeps it awake while Vivianne is working.
chrome.alarms.create('viv-keepalive', { periodInMinutes: 0.5 })
chrome.alarms.onAlarm.addListener(() => connect())
chrome.runtime.onStartup.addListener(connect)
chrome.runtime.onInstalled.addListener(connect)
connect()

chrome.tabs.onRemoved.addListener((id) => {
  attached.delete(id)
  if (lastTab === id) lastTab = null
})
chrome.debugger.onDetach.addListener((src) => {
  attached.delete(src.tabId)
})

// The page's console, kept per tab from the moment the debugger attaches
// (Runtime.enable also replays what the page already logged).
const consoleLog = new Map()
const MAX_CONSOLE = 300

function remember(tabId, entry) {
  const list = consoleLog.get(tabId) ?? []
  list.push({ ...entry, at: Date.now() })
  if (list.length > MAX_CONSOLE) list.shift()
  consoleLog.set(tabId, list)
}

const showArg = (a) => {
  if (a.value !== undefined) return typeof a.value === 'string' ? a.value : JSON.stringify(a.value)
  return a.description ?? a.unserializableValue ?? a.type
}

chrome.debugger.onEvent.addListener((src, method, params) => {
  const tabId = src.tabId
  if (method === 'Runtime.consoleAPICalled') {
    remember(tabId, { level: params.type === 'warning' ? 'warn' : params.type, text: (params.args ?? []).map(showArg).join(' ') })
  } else if (method === 'Runtime.exceptionThrown') {
    const d = params.exceptionDetails ?? {}
    const where = d.url ? ` (${d.url}:${(d.lineNumber ?? 0) + 1})` : ''
    remember(tabId, { level: 'error', text: `${d.exception?.description ?? d.text ?? 'Uncaught exception'}${where}` })
  } else if (method === 'Log.entryAdded') {
    const e = params.entry ?? {}
    remember(tabId, { level: e.level === 'warning' ? 'warn' : e.level, text: `${e.text ?? ''}${e.url ? ` (${e.url})` : ''}`.trim() })
  }
})

// ── Tabs ──────────────────────────────────────────────────────────────────

async function tabFor(params) {
  if (params.tabId) return chrome.tabs.get(params.tabId)
  if (lastTab) {
    try {
      return await chrome.tabs.get(lastTab)
    } catch {
      lastTab = null
    }
  }
  const [active] = await chrome.tabs.query({ active: true, lastFocusedWindow: true })
  if (!active) throw new Error('No tab is open. Use open with a url.')
  lastTab = active.id
  return active
}

async function groupTab(tabId) {
  try {
    const groups = await chrome.tabGroups.query({ title: 'Vivianne' })
    const tab = await chrome.tabs.get(tabId)
    const same = groups.find((g) => g.windowId === tab.windowId)
    if (same) await chrome.tabs.group({ tabIds: tabId, groupId: same.id })
    else {
      const id = await chrome.tabs.group({ tabIds: tabId })
      await chrome.tabGroups.update(id, { title: 'Vivianne', color: 'purple' })
    }
  } catch {
    // Tab groups are a nicety; a browser without them still works.
  }
}

function waitForLoad(tabId, timeoutMs = 20000) {
  return new Promise((resolve) => {
    const done = () => {
      chrome.tabs.onUpdated.removeListener(listener)
      clearTimeout(timer)
      resolve()
    }
    const listener = (id, info) => {
      if (id === tabId && info.status === 'complete') done()
    }
    const timer = setTimeout(done, timeoutMs)
    chrome.tabs.onUpdated.addListener(listener)
    chrome.tabs.get(tabId).then((t) => t.status === 'complete' && done(), done)
  })
}

const describeTab = (t) => ({ tabId: t.id, title: t.title, url: t.url, active: t.active, vivianne: t.groupId !== -1 })

// ── Debugger (CDP) ────────────────────────────────────────────────────────

async function cdp(tabId, method, params = {}) {
  if (!attached.has(tabId)) {
    await chrome.debugger.attach({ tabId }, '1.3')
    attached.add(tabId)
    // Start collecting the console; a page that refuses is still usable.
    await chrome.debugger.sendCommand({ tabId }, 'Runtime.enable').catch(() => {})
    await chrome.debugger.sendCommand({ tabId }, 'Log.enable').catch(() => {})
  }
  return chrome.debugger.sendCommand({ tabId }, method, params)
}

async function inPage(tabId, func, args = []) {
  const [res] = await chrome.scripting.executeScript({ target: { tabId }, func, args })
  return res?.result
}

const KEYS = {
  enter: ['Enter', 13, '\r'],
  tab: ['Tab', 9],
  escape: ['Escape', 27],
  esc: ['Escape', 27],
  backspace: ['Backspace', 8],
  delete: ['Delete', 46],
  space: [' ', 32, ' '],
  arrowup: ['ArrowUp', 38],
  arrowdown: ['ArrowDown', 40],
  arrowleft: ['ArrowLeft', 37],
  arrowright: ['ArrowRight', 39],
  up: ['ArrowUp', 38],
  down: ['ArrowDown', 40],
  left: ['ArrowLeft', 37],
  right: ['ArrowRight', 39],
  pageup: ['PageUp', 33],
  pagedown: ['PageDown', 34],
  home: ['Home', 36],
  end: ['End', 35],
}

async function pressKey(tabId, combo) {
  const parts = combo.toLowerCase().split('+').map((s) => s.trim())
  const keyName = parts.pop()
  const mods = (parts.includes('alt') ? 1 : 0) | (parts.includes('ctrl') || parts.includes('control') ? 2 : 0) | (parts.includes('meta') || parts.includes('cmd') ? 4 : 0) | (parts.includes('shift') ? 8 : 0)
  let [key, code, text] = KEYS[keyName] ?? [keyName.length === 1 ? keyName : keyName, keyName.length === 1 ? keyName.toUpperCase().charCodeAt(0) : 0]
  if (mods & ~8) text = undefined
  else if (!text && key.length === 1) text = mods & 8 ? key.toUpperCase() : key
  const base = { modifiers: mods, key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code }
  await cdp(tabId, 'Input.dispatchKeyEvent', { type: text ? 'keyDown' : 'rawKeyDown', ...base, text })
  await cdp(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', ...base })
}

// ── In-page helpers (run in the page) ─────────────────────────────────────

function pageSnapshot(limit) {
  const ROLE = (el) => {
    const r = el.getAttribute('role')
    if (r) return r
    const t = el.tagName.toLowerCase()
    if (t === 'a') return 'link'
    if (t === 'button' || (t === 'input' && ['button', 'submit', 'reset'].includes(el.type))) return 'button'
    if (t === 'input') return el.type === 'checkbox' ? 'checkbox' : el.type === 'radio' ? 'radio' : 'textbox'
    if (t === 'textarea') return 'textbox'
    if (t === 'select') return 'combobox'
    if (/^h[1-6]$/.test(t)) return 'heading'
    if (el.isContentEditable) return 'textbox'
    return t
  }
  const name = (el) => {
    const label = el.getAttribute('aria-label') || el.getAttribute('title') || el.getAttribute('alt') || ''
    if (label) return label
    if (el.labels && el.labels[0]) return el.labels[0].innerText
    if (el.placeholder) return el.placeholder
    return (el.innerText || el.value || '').replace(/\s+/g, ' ').trim()
  }
  const visible = (el) => {
    const r = el.getBoundingClientRect()
    if (r.width < 2 || r.height < 2) return false
    const s = getComputedStyle(el)
    return s.visibility !== 'hidden' && s.display !== 'none' && s.opacity !== '0'
  }
  document.querySelectorAll('[data-viv-ref]').forEach((e) => e.removeAttribute('data-viv-ref'))
  const sel = 'a[href],button,input:not([type=hidden]),textarea,select,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[role=option],[role=switch],[role=combobox],[role=textbox],[contenteditable=""],[contenteditable=true],[tabindex]:not([tabindex="-1"]),h1,h2,h3,summary'
  const out = []
  let n = 0
  for (const el of document.querySelectorAll(sel)) {
    if (out.length >= limit) break
    if (!visible(el)) continue
    const role = ROLE(el)
    let text = name(el).slice(0, 120)
    if (role === 'heading') {
      out.push(`# ${text}`)
      continue
    }
    n += 1
    el.setAttribute('data-viv-ref', String(n))
    let line = `[${n}] ${role}`
    if (text) line += ` "${text}"`
    const secret = el.getAttribute('data-viv-secret') === '1' || (el.getAttribute('type') || '').toLowerCase() === 'password' || /cc-(number|csc|exp)|one-time-code/.test((el.getAttribute('autocomplete') || '').toLowerCase())
    if ('value' in el && el.value && role !== 'button' && el.tagName !== 'SELECT') line += secret ? ' = "(hidden)"' : ` = "${String(el.value).slice(0, 80)}"`
    if (el.tagName === 'SELECT') line += ` = "${el.options[el.selectedIndex]?.text ?? ''}" options: ${[...el.options].slice(0, 12).map((o) => o.text).join(' | ')}`
    if (el.checked) line += ' checked'
    if (el.disabled) line += ' disabled'
    if (el === document.activeElement) line += ' *focused'
    if (role === 'link' && el.href && !text) line += ` ${el.href}`
    out.push(line)
  }
  const more = out.length >= limit ? `\n… more on the page; scroll and snapshot again, or use find.` : ''
  return `${document.title}\n${location.href}\nScrolled ${Math.round(scrollY)} of ${Math.max(0, document.documentElement.scrollHeight - innerHeight)}px\n\n${out.join('\n')}${more}`
}

function isSensitiveField() {
  const el = document.activeElement
  if (!el) return null
  const type = (el.getAttribute('type') || '').toLowerCase()
  const auto = (el.getAttribute('autocomplete') || '').toLowerCase()
  const hay = [el.name, el.id, el.getAttribute('aria-label'), el.placeholder].filter(Boolean).join(' ').toLowerCase()
  if (type === 'password' || /password|current-password|new-password/.test(auto)) return 'a password'
  if (/one-time-code/.test(auto)) return 'a one-time code'
  if (/cc-(number|csc|exp|name)/.test(auto) || /card.?number|credit.?card|cvv|cvc|security.?code|card.?holder/.test(hay)) return 'payment card details'
  if (/\b(ssn|social.?security|iban|routing.?number|account.?number|passport|tax.?id|aadhaar|pan.?number)\b/.test(hay)) return 'an identity or bank number'
  return null
}

function locate(ref) {
  const el = document.querySelector(`[data-viv-ref="${ref}"]`)
  if (!el) return null
  el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' })
  const r = el.getBoundingClientRect()
  return { x: r.left + r.width / 2, y: r.top + r.height / 2, tag: el.tagName, editable: el.isContentEditable || ['INPUT', 'TEXTAREA'].includes(el.tagName) }
}

function selectOption(ref, value) {
  const el = document.querySelector(`[data-viv-ref="${ref}"]`)
  if (!el || el.tagName !== 'SELECT') return false
  const opt = [...el.options].find((o) => o.value === value || o.text.trim() === value)
  if (!opt) return false
  el.value = opt.value
  el.dispatchEvent(new Event('input', { bubbles: true }))
  el.dispatchEvent(new Event('change', { bubbles: true }))
  return true
}

function findText(query, limit) {
  const q = query.toLowerCase()
  const hits = []
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
  while (walker.nextNode() && hits.length < limit) {
    const t = walker.currentNode.textContent
    const i = t.toLowerCase().indexOf(q)
    if (i >= 0) hits.push(t.slice(Math.max(0, i - 80), i + q.length + 80).replace(/\s+/g, ' ').trim())
  }
  return hits
}

// ── Methods ───────────────────────────────────────────────────────────────

async function handle(method, p) {
  switch (method) {
    case 'tabs': {
      const tabs = await chrome.tabs.query({})
      return { tabs: tabs.map(describeTab), current: lastTab }
    }
    case 'open': {
      if (!p.url) throw new Error('open needs a url')
      const tab = await chrome.tabs.create({ url: p.url, active: p.background !== true })
      await groupTab(tab.id)
      lastTab = tab.id
      await waitForLoad(tab.id)
      return describeTab(await chrome.tabs.get(tab.id))
    }
    case 'select_tab': {
      const tab = await chrome.tabs.update(p.tabId, { active: true })
      lastTab = tab.id
      return describeTab(tab)
    }
    case 'close_tab': {
      const tab = await tabFor(p)
      await chrome.tabs.remove(tab.id)
      return { closed: tab.id }
    }
    case 'navigate': {
      const tab = await tabFor(p)
      if (!p.url) throw new Error('navigate needs a url')
      await chrome.tabs.update(tab.id, { url: p.url })
      await waitForLoad(tab.id)
      return describeTab(await chrome.tabs.get(tab.id))
    }
    case 'back':
    case 'forward':
    case 'reload': {
      const tab = await tabFor(p)
      if (method === 'back') await chrome.tabs.goBack(tab.id)
      else if (method === 'forward') await chrome.tabs.goForward(tab.id)
      else await chrome.tabs.reload(tab.id)
      await waitForLoad(tab.id)
      return describeTab(await chrome.tabs.get(tab.id))
    }
    case 'snapshot': {
      const tab = await tabFor(p)
      lastTab = tab.id
      return { text: await inPage(tab.id, pageSnapshot, [p.limit ?? 300]), tabId: tab.id }
    }
    case 'read': {
      const tab = await tabFor(p)
      const text = await inPage(tab.id, (max) => (document.body?.innerText ?? '').slice(0, max), [p.max ?? 60000])
      return { title: tab.title, url: tab.url, text }
    }
    case 'find': {
      const tab = await tabFor(p)
      return { hits: await inPage(tab.id, findText, [String(p.query ?? ''), p.limit ?? 20]) }
    }
    case 'click': {
      const tab = await tabFor(p)
      const at = await inPage(tab.id, locate, [String(p.ref)])
      if (!at) throw new Error(`No element [${p.ref}] — take a new snapshot`)
      const click = { x: at.x, y: at.y, button: 'left', clickCount: p.double ? 2 : 1 }
      await cdp(tab.id, 'Input.dispatchMouseEvent', { type: 'mouseMoved', x: at.x, y: at.y })
      await cdp(tab.id, 'Input.dispatchMouseEvent', { type: 'mousePressed', ...click })
      await cdp(tab.id, 'Input.dispatchMouseEvent', { type: 'mouseReleased', ...click })
      return { clicked: p.ref }
    }
    case 'type': {
      const tab = await tabFor(p)
      if (p.ref != null) {
        const at = await inPage(tab.id, locate, [String(p.ref)])
        if (!at) throw new Error(`No element [${p.ref}] — take a new snapshot`)
        const click = { x: at.x, y: at.y, button: 'left', clickCount: 1 }
        await cdp(tab.id, 'Input.dispatchMouseEvent', { type: 'mousePressed', ...click })
        await cdp(tab.id, 'Input.dispatchMouseEvent', { type: 'mouseReleased', ...click })
      }
      // Passwords, card numbers, security codes and identity numbers are the
      // user's to type. Checked on the field that has focus, not on what the
      // caller says it is.
      const sensitive = p.secretFill ? null : await inPage(tab.id, isSensitiveField)
      if (sensitive) {
        throw new Error(`That field takes ${sensitive}. Vivianne does not type it: ask the user to enter it themselves.`)
      }
      if (p.clear) {
        await pressKey(tab.id, 'ctrl+a')
        await pressKey(tab.id, 'backspace')
      }
      await cdp(tab.id, 'Input.insertText', { text: String(p.text ?? '') })
      if (p.secretFill) {
        // The value is now in the page and must not be readable from it:
        // dots on screen, and a marker so a snapshot never prints it.
        await inPage(tab.id, () => {
          const el = document.activeElement
          if (!el) return
          el.setAttribute('data-viv-secret', '1')
          el.style.webkitTextSecurity = 'disc'
        })
      }
      if (p.submit) await pressKey(tab.id, 'enter')
      return { typed: p.secretFill ? 0 : String(p.text ?? '').length }
    }
    case 'select': {
      const tab = await tabFor(p)
      const ok = await inPage(tab.id, selectOption, [String(p.ref), String(p.value)])
      if (!ok) throw new Error(`Couldn't choose "${p.value}" in [${p.ref}]`)
      return { selected: p.value }
    }
    case 'key': {
      const tab = await tabFor(p)
      for (let i = 0; i < (p.repeat ?? 1); i++) await pressKey(tab.id, String(p.key))
      return { pressed: p.key }
    }
    case 'scroll': {
      const tab = await tabFor(p)
      const dy = (p.direction === 'up' ? -1 : 1) * (p.amount ?? 600)
      await inPage(tab.id, (y) => scrollBy({ top: y, behavior: 'instant' }), [dy])
      return { scrolled: dy }
    }
    case 'screenshot': {
      const tab = await tabFor(p)
      const shot = await cdp(tab.id, 'Page.captureScreenshot', { format: 'jpeg', quality: 70, fromSurface: true })
      return { image: shot.data, mediaType: 'image/jpeg', url: tab.url, title: tab.title }
    }
    case 'evaluate': {
      const tab = await tabFor(p)
      if (!p.expression) throw new Error('evaluate needs an expression')
      const r = await cdp(tab.id, 'Runtime.evaluate', {
        expression: String(p.expression),
        awaitPromise: true,
        returnByValue: true,
        userGesture: true,
        timeout: 15000,
      })
      if (r.exceptionDetails) {
        const d = r.exceptionDetails
        throw new Error(d.exception?.description ?? d.text ?? 'The expression threw')
      }
      const v = r.result ?? {}
      const value = v.value !== undefined ? v.value : (v.description ?? v.unserializableValue ?? null)
      return { type: v.subtype ?? v.type, value }
    }
    case 'read_console': {
      const tab = await tabFor(p)
      // Attaching starts collection and replays what the page already logged.
      await cdp(tab.id, 'Runtime.enable').catch(() => {})
      await new Promise((r) => setTimeout(r, 150))
      const all = consoleLog.get(tab.id) ?? []
      const messages = all.slice(-(p.limit ?? 100))
      if (p.clear) consoleLog.delete(tab.id)
      return { messages, total: all.length, url: tab.url }
    }
    case 'wait': {
      await new Promise((r) => setTimeout(r, Math.min(15000, p.ms ?? 1000)))
      return { waited: p.ms ?? 1000 }
    }
    case 'done': {
      // Vivianne is finished with the browser: let the "being debugged"
      // banner go.
      for (const id of attached) await chrome.debugger.detach({ tabId: id }).catch(() => {})
      attached.clear()
      return { detached: true }
    }
    default:
      throw new Error(`Unknown method ${method}`)
  }
}
