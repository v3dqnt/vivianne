// Runs inside the app page (see code.cjs). Adds an "Issues" tab to the dock
// group that holds All files / Preview, and draws the panel for `view`:
//   'tab'    – the tab exists, not selected
//   'list'   – selected, open issues listed
//   'detail' – issue #142 open, with "Fix with Vivianne"
//   'fixed'  – #142 closed by the fix
// Drawn with the app's own tokens (theme.ts dark) and type scale (ui/theme.ts).
window.__issues = (view) => {
  const S = window.STORY
  const C = { fg: '#f7f7f7', muted: '#a3a3a3', border: 'rgba(255, 255, 255, 0.09)', card: '#1d1d1d', accent: '#2a2a2a', ok: '#6ec48f', info: '#6ea8e8', selected: 'rgba(247, 247, 247, 0.1)' }
  const svg = (paths, size = 12, sw = 2) =>
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`
  const openIcon = (size) => svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.2" fill="currentColor"/>', size, 1.75)
  const doneIcon = (size) => svg('<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.5 2.5L16 9.5"/>', size, 1.75)

  const preview = document.querySelector('.dv-tab[data-tab-panel-id="preview"]')
  if (!preview) return
  let tab = document.querySelector('.dv-tab[data-tab-panel-id="issues"]')
  if (!tab) {
    tab = preview.cloneNode(true)
    tab.id = 'dv-tab-issues'
    tab.setAttribute('data-tab-panel-id', 'issues')
    tab.setAttribute('aria-label', 'Issues')
    tab.querySelector('svg').outerHTML = openIcon(12)
    tab.querySelector('span').textContent = 'Issues'
    const close = tab.querySelector('button')
    if (close) close.setAttribute('aria-label', 'Close Issues')
    preview.after(tab)
  }
  const group = tab.closest('.dv-groupview')
  const tabs = [...group.querySelectorAll('.dv-tab')]
  const active = view !== 'tab'
  if (active) {
    tabs.forEach((t) => {
      const on = t === tab
      t.classList.toggle('dv-active-tab', on)
      t.classList.toggle('dv-inactive-tab', !on)
      t.setAttribute('aria-selected', String(on))
    })
  }
  if (!active) return

  const label = (l) =>
    `<span style="display:inline-flex;align-items:center;height:18px;padding:0 7px;border-radius:999px;border:1px solid ${C.border};color:${C.muted};font-size:11px;line-height:1">${l}</span>`
  const row = (i, selected) => `
    <div data-issue="${i.number}" style="display:flex;gap:10px;padding:9px 10px;border-radius:8px;${selected ? `background:${C.selected};` : ''}">
      <span style="color:${i.closed ? C.ok : C.info};margin-top:2px">${i.closed ? doneIcon(14) : openIcon(14)}</span>
      <div style="min-width:0;flex:1">
        <div style="font-size:13px;font-weight:500;color:${C.fg};line-height:1.45">${i.title}</div>
        <div style="display:flex;align-items:center;gap:6px;margin-top:4px;font-size:12px;color:${C.muted}">#${i.number} · ${i.meta}</div>
        <div style="display:flex;gap:4px;margin-top:6px">${i.labels.map(label).join('')}</div>
      </div>
    </div>`
  const all = [
    { ...S.issue, meta: 'opened 2 days ago by leo', closed: view === 'fixed' },
    ...S.otherIssues.map((o, k) => ({ ...o, meta: ['opened 4 days ago by priya', 'opened last week by sam'][k] })),
  ]

  let body
  if (view === 'list') {
    body = all.map((i) => row(i, false)).join('')
  } else {
    const i = all[0]
    body = `
      <div style="display:flex;align-items:center;gap:6px;font-size:12px;color:${C.muted};padding:2px 2px 10px">${svg('<path d="M15 6l-6 6 6 6"/>', 12)} All issues</div>
      <div style="padding:0 2px">
        <div style="font-size:16px;font-weight:500;line-height:1.35;color:${C.fg}">${i.title} <span style="color:${C.muted};font-weight:400">#${i.number}</span></div>
        <div style="display:flex;align-items:center;gap:6px;margin-top:8px">
          <span style="display:inline-flex;align-items:center;gap:5px;height:22px;padding:0 9px;border-radius:999px;font-size:12px;font-weight:500;color:#141414;background:${i.closed ? C.ok : C.info}">${i.closed ? doneIcon(12) : openIcon(12)} ${i.closed ? 'Closed' : 'Open'}</span>
          <span style="font-size:12px;color:${C.muted}">${i.meta}</span>
        </div>
        <div style="display:flex;gap:4px;margin-top:10px">${i.labels.map(label).join('')}</div>
        <div style="margin-top:14px;padding:12px;border-radius:10px;background:${C.card};border:1px solid ${C.border};font-size:13px;line-height:1.55;color:${C.fg}">${i.body}</div>
        ${
          i.closed
            ? `<div style="display:flex;gap:8px;margin-top:12px;padding:10px 12px;border-radius:10px;border:1px solid rgba(110,196,143,0.35);background:rgba(110,196,143,0.08);font-size:12.5px;line-height:1.5;color:${C.fg}"><span style="color:${C.ok}">${doneIcon(14)}</span><span>Fixed by Vivianne in <b style="font-weight:500">fix/checkout-safari-tap</b> · 1 file, 1 test · 48 tests pass</span></div>`
            : `<button data-fix style="all:unset;display:inline-flex;align-items:center;gap:7px;margin-top:14px;height:32px;padding:0 14px;border-radius:999px;background:${C.fg};color:#141414;font-size:13px;font-weight:500;cursor:pointer">${svg('<path d="M12 3l1.9 4.6L18.5 9.5l-4.6 1.9L12 16l-1.9-4.6L5.5 9.5l4.6-1.9z"/>', 13, 1.75)} Fix with Vivianne</button>`
        }
      </div>`
  }

  const content = group.querySelector('.dv-content-container')
  content.innerHTML = `
    <div data-issues-panel style="position:absolute;inset:0;padding:10px 12px;overflow:hidden;font-family:Geist, system-ui, sans-serif;color:${C.fg}">
      <div style="display:flex;align-items:center;gap:8px;padding:2px 2px 10px">
        <span style="font-size:13px;font-weight:500">Issues</span>
        <span style="font-size:12px;color:${C.muted}">github.com/northwind/northwind-site</span>
        <span style="flex:1"></span>
        <span style="display:inline-flex;align-items:center;height:22px;padding:0 9px;border-radius:999px;background:${C.accent};font-size:12px">${view === 'fixed' ? 2 : 3} open</span>
      </div>
      <div style="height:1px;background:${C.border};margin:0 -12px 8px"></div>
      ${body}
    </div>`
  content.style.position = 'relative'
}
