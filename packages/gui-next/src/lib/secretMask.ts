/**
 * Hide credentials in text that is only being displayed.
 *
 * A key pasted into a message stays in the transcript, so anyone looking over the user's shoulder (or a screen share) would read it. The shown
 * text keeps the first few characters so you can tell which key it was; the copy button and what the model receives keep the original.
 */

interface Format {
  name: string
  pattern: RegExp
  /** How many leading characters stay visible. */
  keep: number
}

const FORMATS: Format[] = [
  { name: 'private key', pattern: /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)/g, keep: 0 },
  { name: 'Anthropic key', pattern: /\bsk-ant-[A-Za-z0-9_-]{16,}/g, keep: 7 },
  { name: 'API key', pattern: /\bsk-(?:proj-|live-|test-)?[A-Za-z0-9_-]{20,}/g, keep: 3 },
  { name: 'GitHub token', pattern: /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{30,}|\bgithub_pat_[A-Za-z0-9_]{30,}/g, keep: 4 },
  { name: 'AWS key id', pattern: /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/g, keep: 4 },
  { name: 'Slack token', pattern: /\bxox[abprs]-[A-Za-z0-9-]{10,}/g, keep: 4 },
  { name: 'Google key', pattern: /\bAIza[A-Za-z0-9_-]{35}\b/g, keep: 4 },
  { name: 'JWT', pattern: /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g, keep: 3 },
  { name: 'bearer token', pattern: /(?<=\bBearer\s)[A-Za-z0-9._~+/=-]{20,}/g, keep: 0 },
  // `PASSWORD=hunter2hunter2`, `api_key: "…"`: only the value is hidden.
  { name: 'secret value', pattern: /(?<=\b[A-Za-z0-9_]*(?:PASSWORD|PASSWD|SECRET|TOKEN|API_?KEY|ACCESS_?KEY)[A-Za-z0-9_]*["']?\s*[=:]\s*["']?)[^\s"'<>]{8,}/gi, keep: 0 },
]

function maskOne(value: string, keep: number): string {
  const head = value.slice(0, Math.min(keep, Math.max(0, value.length - 8)))
  return `${head}${'•'.repeat(8)}`
}

/** The text with each recognised credential replaced by its first few characters and a run of dots. */
export function maskSecrets(text: string): string {
  let out = text
  for (const f of FORMATS) out = out.replace(f.pattern, (m) => maskOne(m, f.keep))
  return out
}

/** Whether the text contains anything `maskSecrets` would hide. */
export function hasSecrets(text: string): boolean {
  return FORMATS.some((f) => {
    f.pattern.lastIndex = 0
    return f.pattern.test(text)
  })
}
