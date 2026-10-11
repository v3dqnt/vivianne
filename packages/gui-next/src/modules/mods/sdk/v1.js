/*
 * Vivianne mod SDK, v1. Load it first in every mod page:
 *
 *   <script src="/_sdk/v1.js"></script>
 *
 * It gives the page `window.vivianne`, the only way a mod talks to the app.
 * Every call goes to the app over postMessage and is checked against the
 * permissions in the mod's mod.json; a call the mod has no permission for
 * is refused with an error. The page itself runs sandboxed: no app internals,
 * no network beyond the origins it was granted.
 *
 * The SDK also applies the app's theme as CSS variables (--viv-background,
 * --viv-foreground, --viv-primary, --viv-card, --viv-border, --viv-muted-foreground,
 * --viv-radius, --viv-font, --viv-mono …) and keeps them live, and sizes the
 * frame to the page's content in the sidebar and in chat.
 */
;(function () {
  'use strict'
  if (window.vivianne) return

  var pending = {}
  var nextId = 1
  var listeners = {}
  var context = null
  var readyResolve
  var ready = new Promise(function (r) {
    readyResolve = r
  })

  function post(msg) {
    window.parent.postMessage(msg, '*')
  }

  function emit(name, payload) {
    ;(listeners[name] || []).slice().forEach(function (fn) {
      try {
        fn(payload)
      } catch (e) {
        console.error('[vivianne] listener for ' + name + ' failed', e)
      }
    })
  }

  function on(name, fn) {
    ;(listeners[name] = listeners[name] || []).push(fn)
    return function off() {
      listeners[name] = (listeners[name] || []).filter(function (f) {
        return f !== fn
      })
    }
  }

  function call(method, args) {
    return ready.then(function () {
      return new Promise(function (resolve, reject) {
        var id = nextId++
        pending[id] = { resolve: resolve, reject: reject }
        post({ type: 'viv:call', id: id, method: method, args: args === undefined ? null : args })
      })
    })
  }

  function kebab(s) {
    return s.replace(/[A-Z]/g, function (c) {
      return '-' + c.toLowerCase()
    })
  }

  function applyTheme(theme) {
    if (!theme) return
    var root = document.documentElement
    var tokens = theme.tokens || {}
    Object.keys(tokens).forEach(function (k) {
      root.style.setProperty('--viv-' + kebab(k), tokens[k])
    })
    if (theme.radius != null) root.style.setProperty('--viv-radius', theme.radius + 'px')
    if (theme.font) root.style.setProperty('--viv-font', theme.font)
    if (theme.mono) root.style.setProperty('--viv-mono', theme.mono)
    root.setAttribute('data-theme', theme.mode || 'dark')
    root.style.colorScheme = theme.mode || 'dark'
  }

  // Sensible defaults that follow the theme; a mod's own CSS overrides them.
  var base = document.createElement('style')
  base.textContent =
    'html,body{margin:0;background:transparent;color:var(--viv-foreground,#eee);' +
    'font-family:var(--viv-font,system-ui,sans-serif);font-size:13px;line-height:1.5}' +
    'button{font:inherit;color:var(--viv-primary-foreground,#111);background:var(--viv-primary,#eee);' +
    'border:0;border-radius:calc(var(--viv-radius,10px) * .7);padding:4px 10px;cursor:pointer}' +
    'button.secondary{color:var(--viv-foreground);background:var(--viv-secondary,#2a2a2a)}' +
    'input,textarea,select{font:inherit;color:inherit;background:var(--viv-card,#1d1d1d);' +
    'border:1px solid var(--viv-input,#444);border-radius:calc(var(--viv-radius,10px) * .7);padding:4px 8px}' +
    'a{color:var(--viv-primary,#4a9eff)}code,pre{font-family:var(--viv-mono,monospace)}'
  document.documentElement.insertBefore(base, document.documentElement.firstChild)

  window.addEventListener('message', function (e) {
    if (e.source !== window.parent) return
    var m = e.data
    if (!m || typeof m !== 'object') return
    if (m.type === 'viv:init') {
      context = { mod: m.mod, slot: m.slot, data: m.data, permissions: m.permissions || [] }
      applyTheme(m.theme)
      readyResolve(context)
    } else if (m.type === 'viv:result') {
      var p = pending[m.id]
      if (!p) return
      delete pending[m.id]
      if (m.ok) p.resolve(m.value)
      else p.reject(new Error(m.error || 'refused'))
    } else if (m.type === 'viv:event') {
      if (m.name === 'theme') applyTheme(m.payload)
      if (m.name === 'data' && context) context.data = m.payload
      emit(m.name, m.payload)
    }
  })

  // Fit the frame to the content (sidebar sections and chat blocks grow
  // with it; panels ignore this).
  var lastHeight = 0
  function measure() {
    var h = Math.ceil(document.documentElement.scrollHeight)
    if (h !== lastHeight) {
      lastHeight = h
      post({ type: 'viv:resize', height: h })
    }
  }
  if (typeof ResizeObserver !== 'undefined') {
    new ResizeObserver(measure).observe(document.documentElement)
  }
  window.addEventListener('load', measure)

  window.vivianne = Object.freeze({
    version: 1,
    /** Resolves with { mod, slot, data, permissions } once the app has
     * connected. `data` is the chat block's text in a chat component. */
    ready: ready,
    on: on,
    theme: Object.freeze({
      onChange: function (fn) {
        return on('theme', fn)
      },
    }),
    /** Set the frame's height yourself (it otherwise follows the content). */
    resize: function (height) {
      post({ type: 'viv:resize', height: Math.max(0, Math.ceil(height)) })
    },
    /** The mod's own data, up to 1 MB of JSON. */
    storage: Object.freeze({
      get: function () {
        return call('storage.get')
      },
      set: function (value) {
        return call('storage.set', value)
      },
    }),
    /** [notify] A toast in the app. */
    notify: function (text) {
      return call('notify', { text: String(text) })
    },
    chat: Object.freeze({
      /** [chat.read] The open conversation: [{ role, text }]. */
      read: function () {
        return call('chat.read')
      },
      /** [chat.compose] Put text in the message box (the user sends it). */
      compose: function (text) {
        return call('chat.compose', { text: String(text) })
      },
      /** [chat.send] Send a message to Vivianne as the user. */
      send: function (text) {
        return call('chat.send', { text: String(text) })
      },
    }),
    agent: Object.freeze({
      /** [agent.status] What every agent is doing right now. */
      status: function () {
        return call('agent.status')
      },
      /** [agent.status] Called whenever that changes. */
      onStatus: function (fn) {
        call('agent.subscribe').catch(function (e) {
          console.error(e)
        })
        return on('agent.status', fn)
      },
    }),
    files: Object.freeze({
      /** [files.read] A workspace file's text (path relative to the workspace). */
      read: function (path) {
        return call('files.read', { path: String(path) })
      },
      /** [files.read] Names in a workspace folder. */
      list: function (path) {
        return call('files.list', { path: String(path || '.') })
      },
      /** [files.write] Write a workspace file. */
      write: function (path, content) {
        return call('files.write', { path: String(path), content: String(content) })
      },
    }),
    open: Object.freeze({
      /** [open] A link in the user's browser. */
      url: function (url) {
        return call('open.url', { url: String(url) })
      },
      /** [open] One of this mod's panels in the dock. */
      panel: function (id) {
        return call('open.panel', { id: String(id) })
      },
    }),
  })

  post({ type: 'viv:hello', v: 1 })
})()
