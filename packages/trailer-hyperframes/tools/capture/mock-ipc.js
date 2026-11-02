// Injected before the app loads: a fake Tauri bridge. Commands listed in
// window.__MOCK__ return demo data; everything else returns null (the app's
// own "no backend" path). Every command called is logged for discovery.
;(() => {
  const log = (window.__IPC_LOG__ = [])
  let cb = 0
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
      if (cmd.startsWith('plugin:event|')) return cmd.endsWith('listen') ? ++cb : null
      if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:')) return null
      const h = (window.__MOCK__ || {})[cmd]
      if (h === undefined) return null
      return typeof h === 'function' ? h(args) : JSON.parse(JSON.stringify(h))
    },
  }
})()
