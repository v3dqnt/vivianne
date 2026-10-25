import { on } from '../../../ipc'
import { useAgentStore } from '../store/agentStore'

/**
 * A reply typed into a "Vivianne finished" notification
 * (`agent::notifications`): the next message in the chat, sent as if typed
 * here, or queued as a follow-up when a run is already going.
 *
 * Main window only. Only the primary thread is a real conversation (see
 * `agentStore`), so a reply to anything else goes there too.
 */
export function initNotificationReply(): void {
  void on<{ threadId: string; text: string }>('notification:reply', ({ text }) => {
    const agent = useAgentStore.getState()
    if (agent.isBusy) void agent.queueFollowUp(text)
    else void agent.sendMessage(text)
  })
}
