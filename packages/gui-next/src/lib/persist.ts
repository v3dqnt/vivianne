/** Tiny localStorage-backed persistence for UI layout/config — the desktop
 * app is single-user/single-window, so plain localStorage (no sync, no
 * expiry) is sufficient. Never throws: storage can be unavailable (privacy
 * mode, quota) and callers shouldn't have to guard for that. */

export function loadPersisted<T>(key: string, fallback: T): T {
  if (typeof window === 'undefined') return fallback
  try {
    const raw = window.localStorage.getItem(key)
    if (raw === null) return fallback
    const parsed = JSON.parse(raw)
    if (
      typeof fallback === 'object' &&
      fallback !== null &&
      !Array.isArray(fallback) &&
      typeof parsed === 'object' &&
      parsed !== null &&
      !Array.isArray(parsed)
    ) {
      return { ...fallback, ...parsed }
    }
    return parsed as T
  } catch {
    return fallback
  }
}

export function savePersisted(key: string, value: unknown): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // storage unavailable or quota exceeded — losing layout persistence
    // silently is preferable to crashing the app over it
  }
}
