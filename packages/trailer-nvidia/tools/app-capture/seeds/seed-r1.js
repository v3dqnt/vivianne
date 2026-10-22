//@include router-common.js
window.__STUB = { cmd_get_settings: { routing: routing(true) }, cmd_routing_savings: { configured: true, enabled: true } }
const U = await mod('/src/modules/settings/store/uiStore.ts')
U.useSettingsUi.getState().openSettings('providers')
await new Promise((r) => setTimeout(r, 700))
const item = [...document.querySelectorAll('button, div[role="button"], li')].find((b) => /Cost-aware routing/i.test(b.textContent || '') && (b.textContent || '').length < 80)
if (item) item.click()
await new Promise((r) => setTimeout(r, 900))
return item ? "r1" : "r1: routing item not found"
