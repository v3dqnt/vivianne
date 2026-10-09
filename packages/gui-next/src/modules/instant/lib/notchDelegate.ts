import { call, on } from '../../../ipc'
import { useAgentStore } from '../../ai/store/agentStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'
import { useDaysStore } from '../../sessions/store/daysStore'

interface Delegation {
  id: string
  projectId: string
  projectName: string
  text: string
}

/**
 * Work said to the notch, handed to the project's own agent here in the app
 * (`instant::session::delegate`). The project's conversation is where its
 * approvals, plan and transcript already live, so that's where it runs.
 *
 * The chat runs one thing at a time. When it's free, the notch's request
 * switches to the project and is sent. When it's busy with the same project,
 * the request waits its turn as a follow-up. When it's busy with another
 * project, nothing is interrupted: the notch says so instead.
 *
 * Main window only; it runs whether or not the window is showing.
 */
export function initNotchDelegate(): void {
  void on<Delegation>('notch:delegate', ({ id, projectId, projectName, text }) => {
    const reply = (message: string) => void call('cmd_instant_delegate_done', { id, message }).catch(() => {})
    void (async () => {
      try {
        const agent = useAgentStore.getState()
        const projects = useProjectsStore.getState()
        const here = projects.activeProjectId === projectId
        if (agent.isBusy) {
          if (here) {
            await agent.queueFollowUp(text)
            reply(`The ${projectName} lead is busy, so it's next in line.`)
          } else {
            reply(`I'm busy in another project right now. Ask again when that's done, or open Vivianne.`)
          }
          return
        }
        if (!here) projects.setActiveProject(projectId)
        await useDaysStore.getState().ensureToday()
        // Not awaited: it resolves when the whole run ends.
        void useAgentStore.getState().sendMessage(text)
        reply(`On it. The ${projectName} lead has it, and you can follow it up here.`)
      } catch (e) {
        reply(`Couldn't hand that to ${projectName}: ${e instanceof Error ? e.message : String(e)}`)
      }
    })()
  })
}
