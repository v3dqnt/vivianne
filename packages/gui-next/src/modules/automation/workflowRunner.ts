import { create } from 'zustand'
import { call, on } from '../../ipc'
import { useAgentStore } from '../ai/store/agentStore'
import { useDaysStore } from '../sessions/store/daysStore'
import { fill } from './fill'

/** What `automation_fired` carries (`automation::Fired`). */
interface Fired {
  id: string
  name: string
  prompt: string
  steps?: { prompt: string; askFirst?: boolean }[]
  /** The file that set it off, for a folder trigger. */
  file?: string | null
}

/** A step waiting on the user's say-so. */
interface Pending {
  workflow: string
  step: number
  total: number
  text: string
  answer: (run: boolean) => void
}

export const useWorkflowAsk = create<{ pending: Pending | null }>(() => ({ pending: null }))

/** Resolves once the agent isn't busy. */
function idle(): Promise<void> {
  return new Promise((resolve) => {
    if (!useAgentStore.getState().isBusy) return resolve()
    const off = useAgentStore.subscribe((s) => {
      if (!s.isBusy) {
        off()
        resolve()
      }
    })
  })
}

/** After sending a step: wait for the turn it starts to finish. */
async function turnEnds(): Promise<void> {
  // The turn may take a moment to show as busy.
  const until = Date.now() + 5_000
  while (!useAgentStore.getState().isBusy && Date.now() < until) await new Promise((r) => setTimeout(r, 100))
  await idle()
}

async function run(f: Fired): Promise<void> {
  const days = useDaysStore.getState()
  await days.newThread()
  const id = useDaysStore.getState().todayId
  if (id) await useDaysStore.getState().renameThread(id, f.name)

  const steps = [{ prompt: f.prompt, askFirst: false }, ...(f.steps ?? []).map((s) => ({ prompt: s.prompt, askFirst: !!s.askFirst }))]
  for (let i = 0; i < steps.length; i++) {
    const text = fill(steps[i].prompt, f.file)
    if (steps[i].askFirst) {
      // Bring the window up: this is waiting on the user.
      void call('cmd_notch_open_main').catch(() => {})
      const go = await new Promise<boolean>((answer) => {
        useWorkflowAsk.setState({ pending: { workflow: f.name, step: i + 1, total: steps.length, text, answer } })
      })
      useWorkflowAsk.setState({ pending: null })
      if (!go) return
    }
    await useAgentStore.getState().sendMessage(text, [])
    await turnEnds()
  }
}

const queue: Fired[] = []
let draining = false

async function drain(): Promise<void> {
  if (draining) return
  draining = true
  try {
    while (queue.length) {
      // One workflow at a time, and never over a conversation in progress.
      await idle()
      const next = queue.shift()!
      try {
        await run(next)
      } catch (e) {
        console.error('[workflow] failed:', e)
      }
    }
  } finally {
    draining = false
  }
}

/** Start listening for workflows firing. Returns the unsubscribe. */
export function startWorkflowRunner(): () => void {
  const off = on<Fired>('automation_fired', (f) => {
    queue.push(f)
    void drain()
  })
  return () => void off.then((fn) => fn())
}
