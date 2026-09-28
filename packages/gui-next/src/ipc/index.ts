/**
 * The one place that talks to the Rust backend.
 *
 * Every `invoke` in the app goes through a typed function here — no component
 * calls `invoke` directly. The previous frontend spread raw `invoke` calls and
 * their event handling through a single 2000-line provider, which is why its
 * wire contract drifted (it ended up defensively reading both snake_case and
 * camelCase for the same fields). Keeping the boundary in one small typed
 * module means a backend rename breaks the build here, loudly, instead of
 * silently returning undefined somewhere in the UI.
 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** True only inside the real Tauri window. A plain browser tab has no IPC. */
export const hasTauri = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/**
 * `invoke`, but returns `null` instead of throwing when there is no backend
 * (browser preview) — so UI can render a degraded state rather than crashing
 * on mount, which is what the old app did before it grew a guard.
 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!hasTauri()) return null
  let waited = 0
  for (;;) {
    try {
      return await invoke<T>(command, args)
    } catch (err) {
      // The backend is still starting. Tauri creates the windows before the
      // Rust `setup` hook finishes registering state — and `setup` blocks on
      // an OS keychain probe — so the frontend's first calls can land before
      // the state they need exists. They were failing outright: the model
      // list came back empty, today's conversation never opened, and nothing
      // retried, so it stayed that way until a reload.
      //
      // Retried only for this exact error, which Tauri raises for nothing but
      // unregistered state. Any other failure is a real one and throws at
      // once. The bound means a genuinely broken startup still surfaces.
      if (isStartupRace(err) && waited < STARTUP_WAIT_LIMIT_MS) {
        const delay = Math.min(250, 25 * 2 ** Math.min(4, Math.floor(waited / 100)))
        await new Promise((r) => setTimeout(r, delay))
        waited += delay
        continue
      }
      console.error(`[ipc] ${command} failed:`, err)
      throw err
    }
  }
}

/** Long enough for any real startup; short enough that a broken one surfaces. */
const STARTUP_WAIT_LIMIT_MS = 20_000

function isStartupRace(err: unknown): boolean {
  const message = typeof err === 'string' ? err : err instanceof Error ? err.message : ''
  return message.includes('state not managed for field')
}

/**
 * Subscribe to a backend event. Returns the unsubscribe function — callers
 * MUST call it on unmount. (Leaked listeners were a real bug class in the old
 * app; keeping subscription and teardown in one returned value makes the
 * correct thing the easy thing.)
 */
export async function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!hasTauri()) return () => {}
  return listen<T>(event, (e) => handler(e.payload))
}
