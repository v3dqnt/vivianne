//@include mods-common.js
// b4: its switch, pressed: Vivianne asks before the mod runs.
window.__STUB.cmd_mods_list = [modInfo('agent-board', 'needsApproval'), modInfo('checklist', 'on')]
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: build })
const U = await mod('/src/modules/settings/store/uiStore.ts')
U.useSettingsUi.getState().openSettings('mods')
await new Promise((r) => setTimeout(r, 900))
// Its switch asks for consent first.
const SW = '[role=switch], input[type=checkbox]'
const row = [...document.querySelectorAll('.MuiStack-root, div')].filter((d) => /Agent board/.test(d.textContent || '') && d.querySelector(SW)).sort((a, b) => a.textContent.length - b.textContent.length)[0]
const sw = row && row.querySelector(SW)
if (sw) sw.click()
await new Promise((r) => setTimeout(r, 900))
return sw ? 'b4' : 'b4: switch not found'
