import { create } from 'zustand'
import { call } from '../../../ipc'

export interface AccountUser {
  id: string
  email: string | null
  displayName: string | null
}

interface AccountStoreState {
  user: AccountUser | null
  loading: boolean
  /** Set only by an actual failed call (bad password, network error, ...) —
   * "not signed in yet" is not an error, so `init` never sets this. */
  error: string | null
  initialized: boolean
  init: () => Promise<void>
  /** Opens vivianne.dev/sign-in in the system browser and waits for it to
   * hand back a session — see `cmd_account_sign_in_with_browser`'s doc
   * comment for why sign-in isn't an in-app form. */
  signInWithBrowser: () => Promise<void>
  signOut: () => Promise<void>
  updateDisplayName: (displayName: string) => Promise<void>
}

/** Account (Supabase-backed login), separate from `ai/store/*` — those
 * stores are about the LLM provider/session the agent runs against; this is
 * "who's signed into Vivianne itself," an orthogonal concern. */
export const useAccountStore = create<AccountStoreState>((set) => ({
  user: null,
  loading: false,
  error: null,
  initialized: false,

  init: async () => {
    set({ loading: true })
    try {
      const user = await call<AccountUser | null>('cmd_account_get_session')
      set({ user: user ?? null, initialized: true })
    } finally {
      set({ loading: false })
    }
  },

  signInWithBrowser: async () => {
    set({ loading: true, error: null })
    try {
      const user = await call<AccountUser>('cmd_account_sign_in_with_browser')
      set({ user: user ?? null })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
      throw err
    } finally {
      set({ loading: false })
    }
  },

  signOut: async () => {
    set({ loading: true, error: null })
    try {
      await call('cmd_account_sign_out')
      set({ user: null })
    } finally {
      set({ loading: false })
    }
  },

  updateDisplayName: async (displayName) => {
    set({ error: null })
    try {
      const user = await call<AccountUser>('cmd_account_update_profile', { displayName })
      set({ user: user ?? null })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
      throw err
    }
  },
}))
