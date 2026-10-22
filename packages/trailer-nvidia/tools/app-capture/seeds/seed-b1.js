//@include mods-common.js
// b1: the mod is written; nothing runs until it is turned on.
window.__STUB.cmd_mods_list = [modInfo('agent-board', 'needsApproval')]
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: build })
await new Promise((r) => setTimeout(r, 500))
// Its steps unfolded: the guide it read and the two files it wrote.
const fold = [...document.querySelectorAll('button, [role=button]')].find((b) => /Edited 2 files/.test(b.textContent || ''))
if (fold) fold.click()
await new Promise((r) => setTimeout(r, 600))
return 'b1'
