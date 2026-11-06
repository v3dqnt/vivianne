/**
 * What a 24/7 agent's approval request shows in its chat: a small card with
 * what it wants to do, the details that matter for deciding, and how risky
 * it is. Pure: the card component draws whatever this returns.
 */

/** Every kind of card, for the Widget Lab to show each one. */
export const WIDGET_KINDS = ['payment', 'email', 'message', 'shell', 'file', 'browser', 'screen', 'generic'] as const
export type WidgetKind = (typeof WIDGET_KINDS)[number]

/** How careful the card asks you to be: high is money or secrets. */
export type Danger = 'low' | 'medium' | 'high'

export interface WidgetField {
  label: string
  value: string
  mono?: boolean
}

export interface ApprovalWidget {
  kind: WidgetKind
  title: string
  danger: Danger
  fields: WidgetField[]
  /** Longer text shown as a quote under the fields (a message's body). */
  body?: string
  /** Why it is asking, when it said. */
  note?: string
}

export interface ApprovalInput {
  toolName: string
  args: Record<string, unknown>
  reason: string | null
}

/** The parts of a card the vault fills, as a person names them. */
const CARD_PARTS: Record<string, string> = { number: 'card number', expiry: 'expiry date', cvv: 'security code', name: 'name on card' }

const FILE_TOOLS = ['write', 'edit', 'apply_patch', 'notebook_edit', 'create_document', 'edit_document']

/** What a browser action is, said as a person would. */
const BROWSER_VERBS: Record<string, string> = {
  open: 'open a page in your browser',
  navigate: 'open a page in your browser',
  click: 'click in your browser',
  type: 'type in your browser',
  select: 'pick an option in your browser',
  key: 'press a key in your browser',
  evaluate: 'run a script on a page in your browser',
}

const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : String(v))

/** Longest a field may be; past it the middle goes, as the start and the
 * end are what tell you what a command or a path is. */
const MAX_FIELD = 300

function clip(v: string, max: number): string {
  if (v.length <= max) return v
  const tail = Math.floor((max - 1) / 3)
  return `${v.slice(0, max - 1 - tail)}…${v.slice(v.length - tail)}`
}

/** No blank rows, nothing too long to read. */
function tidy(w: ApprovalWidget): ApprovalWidget {
  const fields = w.fields.filter((f) => f.value.trim() !== '').map((f) => ({ ...f, value: clip(f.value, MAX_FIELD) }))
  const out: ApprovalWidget = { ...w, fields }
  if (!w.body?.trim()) delete out.body
  return out
}

export function describeApproval(a: ApprovalInput, agent: string): ApprovalWidget {
  const w = describe(a, agent)
  if (a.reason?.trim()) w.note = a.reason.trim()
  return tidy(w)
}

/** `mcp__notion__create_page` → `Notion: create page`; `make_thing` → `make thing`. */
function toolLabel(name: string): string {
  const words = (s: string) => s.replace(/[_-]+/g, ' ').trim()
  const m = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(name)
  if (!m) return words(name)
  const app = words(m[1])
  return `${app.charAt(0).toUpperCase()}${app.slice(1)}: ${words(m[2])}`
}

const SHELL_TOOLS = ['bash', 'powershell', 'terminal', 'run_script', 'sandbox_execute']

function describe(a: ApprovalInput, agent: string): ApprovalWidget {
  const g = a.args
  if (a.toolName === 'vault' && str(g.field) in CARD_PARTS) {
    return {
      kind: 'payment',
      title: `${agent} wants to pay with your card`,
      danger: 'high',
      fields: [
        { label: 'Card', value: str(g.secret) },
        { label: 'Fills', value: CARD_PARTS[str(g.field)] },
      ],
    }
  }
  if (a.toolName === 'phone') {
    const call = str(g.action) === 'call'
    return {
      kind: 'message',
      title: call ? `${agent} wants to make a call` : `${agent} wants to send a text`,
      danger: 'medium',
      fields: [{ label: 'To', value: str(g.to) }],
      body: call ? undefined : str(g.text),
    }
  }
  if (FILE_TOOLS.includes(a.toolName)) {
    return { kind: 'file', title: `${agent} wants to change a file`, danger: 'medium', fields: [{ label: 'File', value: str(g.path), mono: true }] }
  }
  if (a.toolName === 'user_browser_driver') {
    const action = str(g.action)
    const verb = BROWSER_VERBS[action] ?? 'use your browser'
    return {
      kind: 'browser',
      title: `${agent} wants to ${verb}`,
      danger: 'low',
      fields: [
        { label: 'Page', value: str(g.url) },
        { label: 'Text', value: str(g.text) },
      ],
    }
  }
  if (a.toolName === 'dip' || a.toolName === 'computer') {
    return {
      kind: 'screen',
      title: `${agent} wants to use your screen`,
      danger: 'medium',
      fields: [
        { label: 'Action', value: str(g.action).replace(/_/g, ' ') },
        { label: 'On', value: str(g.target ?? g.element ?? g.app) },
      ],
    }
  }
  if (a.toolName === 'email') {
    return {
      kind: 'email',
      title: `${agent} wants to send an email`,
      danger: 'medium',
      fields: [
        { label: 'To', value: str(g.to) },
        { label: 'Subject', value: str(g.subject) },
      ],
      body: str(g.text),
    }
  }
  if (SHELL_TOOLS.includes(a.toolName)) {
    return { kind: 'shell', title: `${agent} wants to run a command`, danger: 'medium', fields: [{ label: 'Command', value: str(g.command ?? g.code ?? g.script), mono: true }] }
  }
  // Anything else: its name, and its first few plain details.
  const fields = Object.entries(g)
    .filter(([, v]) => ['string', 'number', 'boolean'].includes(typeof v))
    .slice(0, 4)
    .map(([k, v]) => ({ label: `${k.charAt(0).toUpperCase()}${k.slice(1).replace(/_/g, ' ')}`, value: str(v) }))
  return { kind: 'generic', title: `${agent} wants to use ${toolLabel(a.toolName)}`, danger: 'medium', fields }
}
