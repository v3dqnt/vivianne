/**
 * Keep a half-answered question across a reload or a remount.
 *
 * The agent's question is waiting on the backend, so losing the selections already made (a window reload, switching threads and back) means
 * answering again from scratch. Selections are kept per request id in session storage and dropped once the question is answered.
 */

const KEY = (requestId: string) => `viv.question.${requestId}`

/** The saved selections for this request, if they still fit the question (same number of questions). */
export function loadQuestionDraft(requestId: string, questionCount: number, storage: Pick<Storage, 'getItem'> = sessionStorage): string[][] | null {
  try {
    const raw = storage.getItem(KEY(requestId))
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed) || parsed.length !== questionCount) return null
    return parsed.map((s) => (Array.isArray(s) ? s.filter((l): l is string => typeof l === 'string') : []))
  } catch {
    return null
  }
}

export function saveQuestionDraft(requestId: string, selections: string[][], storage: Pick<Storage, 'setItem'> = sessionStorage): void {
  try {
    storage.setItem(KEY(requestId), JSON.stringify(selections))
  } catch {
    // Storage unavailable: the draft just doesn't survive.
  }
}

export function clearQuestionDraft(requestId: string, storage: Pick<Storage, 'removeItem'> = sessionStorage): void {
  try {
    storage.removeItem(KEY(requestId))
  } catch {
    // Nothing to clear.
  }
}
