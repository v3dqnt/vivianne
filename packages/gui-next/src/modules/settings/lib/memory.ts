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
}
