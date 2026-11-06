import { WIDGET_LAB } from '../modules/team/lib/widgetLab'
import { PALETTES } from '../palettes'

/** Sample props for the components that can't render without any, by name. Anything not listed renders with none. */

const text = (t: string) => [{ type: 'text', text: t }]
const toolResult = (toolName: string, out: string, isError: boolean | null = false) => ({
  type: 'toolResult', toolCallId: `${toolName}-1`, toolName, content: text(out), details: null, isError, timestamp: 0,
})

const SUMMARY = `<compacted_history>
The user asked for threads on their own branches. The agent added worktrees, checkpoints and a project rail, then tested each.
</compacted_history>`

const PIXEL = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='

const BOT_STEPS = [
  { id: '1', tool: 'read_file', label: 'Read ProjectTools.tsx', state: 'done' },
  { id: '2', tool: 'edit_file', label: 'Edit the rail layout', state: 'running' },
  { id: '3', tool: 'bash', label: 'Run the tests', state: 'error', detail: 'exit code 1' },
]

const AGENT = { id: 'a1', name: 'Mira', role: 'Research lead', status: 'active', reportsTo: null, model: null, allowedTools: null, owns: [], color: '#3e63dd', face: null }
const ROOT = 'C:/demo'
const noop = () => {}

export const FIXTURES: Record<string, Record<string, unknown>> = {
  CodeBlock: { code: 'function greet(name: string) {\n  return `Hello, ${name}`\n}', lang: 'ts' },
  FileLink: { path: 'src/modules/team/components/ProjectTools.tsx', label: 'ProjectTools.tsx' },
  FileCard: { path: 'src/modules/team/components/ProjectTools.tsx' },
  Markdown: { text: '## A heading\n\nSome **bold**, some `code`, and a list:\n\n- one\n- two\n\n> a quote' },
  ReportMarkdown: { text: '# Findings\n\n| Area | Result |\n|---|---|\n| Rail | Done |\n| Gallery | Done |\n\n- All tests pass' },
  HandoffDivider: { text: '<handoff date="2026-11-06" kind="session">Continuing from the last session.</handoff>' },
  SummaryDivider: { text: SUMMARY },
  ResumeDivider: { text: '<long_task>Pick up the migration where it stopped.</long_task>' },
  PairActivityDivider: { text: '<pair_activity partner="Alex">Alex edited three files.</pair_activity>' },
  PlanNoteDivider: { text: '<plan_approved>Plan approved: build the gallery.</plan_approved>' },
  ImageBlock: { block: { type: 'image', base64: PIXEL, mediaType: 'image/png' } },
  InteractiveBlock: { code: '{"type":"choice","question":"Pick one","options":["A","B"]}' },
  BioBlock: { code: '>seq\nMKTAYIAKQRQISFVKSHFSRQ' },
  ChemBlock: { code: 'CCO' },
  PlotBlock: { code: '{"type":"line","x":[1,2,3],"y":[2,4,3]}' },
  PhysicsBlock: { code: '{"type":"pendulum"}' },
  Mol3DBlock: { code: 'CCO' },
  SciBlock: { code: 'CCO', lang: 'smiles' },
  MathSpan: { tex: 'E = mc^2', display: true },
  Sources: { sources: [{ url: 'https://example.com/a', title: 'First source' }, { url: 'https://example.com/b', title: 'Second source' }] },
  StatusMark: { status: 'modified' },
  PrChip: { pr: { number: 12, title: 'Add the gallery', state: 'open', draft: false, url: 'https://example.com/pull/12', additions: 40, deletions: 3, checks: 'passing', failing: [] } },
  ToolCallCard: { message: toolResult('read_file', 'line one\nline two') },
  ToolGroup: { tools: [toolResult('read_file', 'ok'), toolResult('bash', 'exit 0')], thoughts: 1, children: 'Group body' },
  StepList: { steps: BOT_STEPS },
  StepsToggle: { steps: BOT_STEPS },
  ProjectMark: { project: { id: 'p1', name: 'Project Versa', color: '#3e63dd', icon: null }, size: 28 },
  Avatar: { name: 'Mira', color: '#3e63dd', size: 36 },
  AgentFace: { name: 'Mira', color: '#3e63dd', size: 56, state: 'idle' },
  Choices: { options: ['One', 'Two', 'Three'], value: 'One', onChange: () => {} },
  ListInput: { value: ['alpha', 'beta'], onChange: () => {}, placeholder: 'Add an item' },
  ChangelogBody: { entries: [{ version: '1.9.1-rdv2', date: '2026-11-06', sections: [{ title: 'Changed', items: ['A new project rail', 'One branch per thread'] }] }] },
  RunSummary: { ledger: { key: 'k', thread: 't', workspace: null, objective: 'Build the gallery', status: 'done', capUsd: null, deadlineMs: null, startedAt: 0, updatedAt: 42000, spentUsd: 0.04, inputTokens: 5200, outputTokens: 1800, steps: 9, model: 'anthropic/claude-sonnet', waitUntil: null, statusReason: null, milestones: [], todos: [] } },
  ApprovalWidget: { widget: { kind: 'email', ...WIDGET_LAB[2].approval }, state: 'pending', accent: '#3e63dd' },
  MentionRow: { active: true, icon: '#', tint: '#3e63dd', title: 'ProjectTools.tsx', subtitle: 'src/modules/team', tag: 'file', onSelect: noop, onHover: noop },
  LiveAvatar: { root: ROOT, agent: AGENT, size: 36 },
  BotChip: { root: ROOT, agent: AGENT, onOpen: noop },
  LiveRunCard: { agent: AGENT, steps: BOT_STEPS },
  AgentMessenger: { root: ROOT, agent: AGENT, team: [AGENT], status: 'idle' },
  AgentPage: { root: ROOT, agent: AGENT, onBack: noop, onEdit: noop },
  BotPage: { root: ROOT, agent: AGENT, onBack: noop, onEdit: noop },
  ContactTab: { root: ROOT, agent: AGENT },
  AgentSettingsPanel: { root: ROOT, agent: AGENT, onClose: noop },
  RoutineDialog: { agent: AGENT, initial: null, onSave: async () => {}, onClose: noop },
  FaceStage: { name: 'Mira', color: '#3e63dd', face: null, size: 96 },
  FaceControls: { name: 'Mira', color: '#3e63dd', face: null, onChange: noop },
  FaceEditor: { name: 'Mira', color: '#3e63dd', face: null, onChange: noop },
  ProviderRow: { provider: { id: 'anthropic', label: 'Anthropic' } },
  ThemeBuilder: { base: PALETTES[0], editing: false, onClose: noop },
  ThreadRow: { thread: { id: 's1', name: 'Build the gallery', messages: [], created_at: '2026-11-06', workspace_folders: [ROOT] }, active: false, onOpen: noop },
  ReviewPanel: { open: true, onClose: noop, files: [{ path: 'src/a.ts', status: 'modified', additions: 4, deletions: 1 }], root: ROOT, readOnly: false, busy: false, onUndo: noop, note: null },
  CheckpointCard: { checkpoint: { index: 3, at: 0, items: [{ content: 'Add the rail', status: 'completed' }], positions: [2], total: 5, done: 2, files: ['ProjectTools.tsx'], elapsedMs: 31000, costUsd: 0.02 } },
}

/** Names that look like components but are constants or helpers, so the gallery skips them. */
FIXTURES.SciBlock = { label: 'Molecule', title: 'Ethanol', source: 'CCO', error: null, loading: false, render: () => null, getSvg: () => '', getPng: async () => '', hint: '', minHeight: 80 }
FIXTURES.MessageBubble = { message: { type: 'assistant', content: text('A reply from the assistant, with **markdown**.'), stopReason: 'stop', errorMessage: null, timestamp: 0 }, index: 0 }
FIXTURES.DocumentViewer = { path: 'C:/demo/notes.md' }
FIXTURES.UnifiedMentionMenu = { items: [{ kind: 'file', path: 'src/App.tsx', name: 'App.tsx' }], activeIndex: 0, onSelect: noop, onHover: noop }
FIXTURES.ChatComposer = { value: '', onChange: noop, onSubmit: noop, placeholder: 'Do anything', attachments: true }

export const NOT_COMPONENTS = /^[A-Z][A-Z0-9_]*$/
