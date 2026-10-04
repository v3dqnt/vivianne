chrome.storage.session.get('connected').then(({ connected }) => {
  document.getElementById('root').classList.toggle('on', !!connected)
  document.getElementById('title').textContent = connected ? 'Connected to Vivianne' : 'Not connected'
  document.getElementById('body').textContent = connected
    ? 'Vivianne can use this browser. Tabs it opens are grouped as “Vivianne”.'
    : 'Open the Vivianne app — this connects on its own when it is running.'
})
