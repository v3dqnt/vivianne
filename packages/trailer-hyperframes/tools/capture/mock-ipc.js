// Injected before the app loads: a fake Tauri bridge. Commands listed in
// window.__MOCK__ return demo data; everything else returns null (the app's
// own "no backend" path). Every command called is logged for discovery.
;(() => {
  const log = (window.__IPC_LOG__ = [])
  let cb = 0
  const listeners = {}
  /** Fire a backend event at the app, as Tauri would. */
  window.__emit = (event, payload) => {
    for (const id of listeners[event] || []) window[`_${id}`]?.({ event, id, payload })
  }
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback: (fn) => {
      const id = ++cb
      window[`_${id}`] = fn
      return id
    },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      log.push(cmd)
      if (cmd === 'plugin:event|listen') {
        ;(listeners[args.event] = listeners[args.event] || []).push(args.handler)
        return args.handler
      }
      if (cmd.startsWith('plugin:event|')) return null
      if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:')) return null
      const h = (window.__MOCK__ || {})[cmd]
      if (h === undefined) return null
      return typeof h === 'function' ? h(args) : JSON.parse(JSON.stringify(h))
    },
  }
})()
