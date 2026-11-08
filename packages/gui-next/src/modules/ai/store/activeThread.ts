import { create } from 'zustand'

/** The backend's primary thread (`AgentRegistry::resolve` treats '' as it). */
export const PRIMARY_THREAD_ID = ''

/** Which thread is on screen. Its own module so the model and workspace
 * stores can read it without importing the agent store. */
export const useActiveThread = create(() => ({ threadId: PRIMARY_THREAD_ID }))

export const activeThreadId = () => useActiveThread.getState().threadId
