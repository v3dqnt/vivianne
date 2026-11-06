/**
 * Messages the app writes in the user's name (day handoffs, compaction
 * summaries, resumes, plan answers, team traffic): how to tell them from what
 * the person typed. Pure, so the chats and their tests share one definition.
 */

/** Matches `agent::days::HANDOFF_TAG`. */
export const HANDOFF_TAG = '<previous_day'

export function isHandoffText(text: string): boolean {
  return text.trimStart().startsWith(HANDOFF_TAG)
}

/** Matches `agent::conversation::SUMMARY_TAG` — the note compaction puts at
 * the front of the model's context in place of the messages it removed. */
const SUMMARY_TAG = '<compacted_history>'

export function isSummaryText(text: string): boolean {
  return text.trimStart().startsWith(SUMMARY_TAG)
}

/** Matches `multiplayer::collab::PAIR_ACTIVITY_TAG`: what the partner's
 * agent did, added to the conversation right before a run so this agent
 * knows. Shown as a divider — it isn't something the user said. */
export function isPairActivityText(text: string): boolean {
  return text.trimStart().startsWith('<pair_activity')
}

/** A user-role message the app wrote, not the person: day handoffs,
 * compaction summaries, resumes, plan notes, team messages. Never shown as
 * "what you asked". */
export function isAppText(text: string): boolean {
  return /^\s*<(previous_day|compacted_history|pair_activity|long_task_resumed|long_task|plan_feedback|plan_approved|team_message|user_comment)\b/.test(text)
}

/** Matches `run_ledger::cmd_long_task_resume_prompt`: the message that picks
 * a long task back up. It is the app speaking, not the user. */
export function isResumeText(text: string): boolean {
  return text.trimStart().startsWith('<long_task_resumed>')
}

/** Plan review traffic: the comments sent back on a plan, or its approval.
 * Written by the app on the user's behalf (see `plan/store/planStore.ts` and
 * `plans::cmd_plan_approve`), so shown as a marker, not a wall of XML. */
export function isPlanNoteText(text: string): boolean {
  const t = text.trimStart()
  return t.startsWith('<plan_feedback') || t.startsWith('<plan_approved')
}
