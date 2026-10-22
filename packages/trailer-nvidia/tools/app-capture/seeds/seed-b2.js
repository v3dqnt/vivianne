//@include mods-common.js
// b2: Settings → Mods, the new Agent board waiting on its one permission.
window.__STUB.cmd_mods_list = [modInfo('agent-board', 'needsApproval'), modInfo('checklist', 'on')]
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: build })
const U = await mod('/src/modules/settings/store/uiStore.ts')
U.useSettingsUi.getState().openSettings('mods')
await new Promise((r) => setTimeout(r, 900))
return 'b2'
