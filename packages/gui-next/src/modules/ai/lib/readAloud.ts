import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { speak, stopSpeaking } from '../../instant/lib/instantTts'
import { useAgentStore } from '../store/agentStore'
import { contentToText } from './types'
import { toSpeakable } from './speakable'

/** Which message is being read, so its button can show Stop. */
export const useReading = create<{ id: number | null }>(() => ({ id: null }))

/** Read an assistant message aloud, or stop if it's the one being read. */
export async function toggleRead(id: number, markdown: string): Promise<void> {
  if (useReading.getState().id === id) {
    stopSpeaking()
    useReading.setState({ id: null })
    return
  }
  const text = toSpeakable(markdown)
  if (!text) return
  useReading.setState({ id })
  await speak(text)
  if (useReading.getState().id === id) useReading.setState({ id: null })
}

// ── Reading replies to a spoken prompt ──────────────────────────────────────

const KEY = 'viv.readAloud'
export const useReadAloudPref = create<{ whenDictated: boolean; set: (v: boolean) => void }>((set) => ({
  whenDictated: loadPersisted<{ whenDictated?: boolean }>(KEY, {}).whenDictated ?? false,
  set: (v) => {
    set({ whenDictated: v })
    savePersisted(KEY, { whenDictated: v })
  },
}))

let dictated = false
let armed = false

/** The draft now holds words that were spoken. */
export function markDictated(): void {
  dictated = true
}

/** The draft was sent: if it was spoken, its reply gets read out. */
export function noteSent(): void {
  armed = dictated
  dictated = false
}

/** When a turn started by a spoken prompt ends, read the answer. */
export function startAutoReadAloud(): () => void {
  let wasBusy = false
  return useAgentStore.subscribe((s) => {
    if (wasBusy && !s.isBusy && armed) {
      armed = false
      if (useReadAloudPref.getState().whenDictated) {
        const last = [...s.messages].reverse().find((m) => m.type === 'assistant' && contentToText(m.content).trim())
        if (last && last.type === 'assistant') void speak(toSpeakable(contentToText(last.content)))
      }
    }
    wasBusy = s.isBusy
  })
}
