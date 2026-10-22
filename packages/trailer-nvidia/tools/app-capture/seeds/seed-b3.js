//@include mods-common.js
// b3: mods on — the Agent board in the dock, and a checklist that
// the Checklist mod draws in the chat.
const PS = await mod('/src/modules/dock/store/panelStore.ts')
const MS = await mod('/src/modules/mods/store/modsStore.ts')
window.__STUB.cmd_mods_list = [modInfo('agent-board', 'on'), modInfo('checklist', 'on')]
await MS.useModsStore.getState().refresh('C:/dev/orders-api')
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  ...build,
  user("What's left before the release?", T1 + 60000),
  say('Three things, one done:\n\n```checklist\n- [x] Backfill orders_v2 (dry run passed)\n- [ ] Switch reads behind the flag\n- [ ] Tag 2.4.0 and publish the notes\n```', T1 + 70000),
] })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openCustomPanel('agent-board/main')
await new Promise((r) => setTimeout(r, 2500))
return 'b3'
