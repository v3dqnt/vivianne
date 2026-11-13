/** Mirrors `agent::memory::MemoryState` — `#[serde(rename_all = "camelCase")]`. */
export interface MemoryState {
  enabled: boolean
  autoConsolidate: boolean
  createdAt: number
  updatedAt: number
  lastInjectedAt: number | null
  extractionEnabled: boolean
  lastExtractionAt: number | null
  lastPersonaSynthesisAt: number | null
  /** When the last tidy-up pass (re-embed, episodes, dedupe, forgetting) ran. */
  lastSleepAt?: number | null
  /** The project's pages: named questions whose answers are kept up to date. */
  pages?: { name: string; question: string }[]
  /** Refuse text that holds a secret instead of saving it with the secret hidden. */
  blockSecrets?: boolean
  /** Secrets and card numbers memory defense has hidden or refused. */
  redactedCount?: number
}

export interface HindsightConfig {
  enabled: boolean
  url: string
  bank: string
  retain: boolean
  hasKey?: boolean
}

/** Mirrors `agent::memory_sleep::SleepReport`. */
export interface SleepReport {
  reembedded: number
  episodes: number
  merged: number
  archived: number
  observations?: number
  pages?: number
}

/** Mirrors `agent::memory::MemoryStatus` — `#[serde(rename_all = "camelCase")]`. */
export interface MemoryStatus {
  root: string
  state: MemoryState
  entryCount: number
  estimatedTokens: number
  bytes: number
  preview: string
}

/** Mirrors `agent::memory::MemoryEntry` — `#[serde(rename_all = "camelCase")]`. */
export interface MemoryEntry {
  id: string
  key: string
  text: string
  kind: string
  sessionId: string | null
  createdAt: number
  confidence: number
  /** From the user's own memory (shared by every project), not this project's. */
  userWide?: boolean
}
