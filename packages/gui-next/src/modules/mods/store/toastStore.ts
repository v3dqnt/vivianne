import { create } from 'zustand'

/** Short-lived notices at the edge of the window (mods' `notify`, and any
 * other brief confirmation). */
interface ToastState {
  items: { id: number; text: string }[]
  push: (text: string) => void
  dismiss: (id: number) => void
}

let seq = 1

export const useToastStore = create<ToastState>((set) => ({
  items: [],
  push: (text) => {
    const id = seq++
    set((s) => ({ items: [...s.items.slice(-3), { id, text }] }))
    window.setTimeout(() => set((s) => ({ items: s.items.filter((t) => t.id !== id) })), 5000)
  },
  dismiss: (id) => set((s) => ({ items: s.items.filter((t) => t.id !== id) })),
}))

export const toast = (text: string) => useToastStore.getState().push(text)
