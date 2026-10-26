/** Which thread is on screen (`''` is the primary). Its own module so the
 * model and workspace stores can read it without importing the agent store. */
let active = ''
const switched = new Set<() => void>()

export const activeThreadId = () => active

export function onThreadSwitch(f: () => void): () => void {
  switched.add(f)
  return () => void switched.delete(f)
}

export function switchThread(threadId: string) {
  if (threadId === active) return
  active = threadId
  switched.forEach((f) => f())
}
