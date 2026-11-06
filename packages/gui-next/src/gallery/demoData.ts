/** A believable session for the gallery's scenes. */
import type { AgentMessage } from '../modules/ai/lib/types'

const text = (t: string) => [{ type: 'text', text: t }]
const call = (id: string, name: string, args: Record<string, unknown>) => ({ type: 'toolCall', id, name, arguments: args })
const result = (id: string, name: string, out: string, isError = false) => ({
  type: 'toolResult', toolCallId: id, toolName: name, content: text(out), details: null, isError, timestamp: 0,
})

export const DEMO_MESSAGES = [
  { type: 'user', content: text('Move the project selector into the rail and merge workspace with project.'), timestamp: 1 },
  {
    type: 'assistant',
    content: [
      ...text('I will read the rail first, then change it.'),
      call('c1', 'read_file', { path: 'src/modules/team/components/ProjectTools.tsx' }),
      call('c2', 'grep', { pattern: 'ProjectPill' }),
    ],
    stopReason: 'toolUse', errorMessage: null, timestamp: 2,
  },
  result('c1', 'read_file', 'export function ProjectTools() {\n  // ...\n}'),
  result('c2', 'grep', 'ProjectPicker.tsx:29: export function ProjectPill\nComposer.tsx:354: <ProjectPill />'),
  {
    type: 'assistant',
    content: [
      ...text('The picker now lives in the rail. Here is what changed:\n\n- **Project** and **Workspace** are one section\n- The composer no longer shows a project pill\n\n```ts\n<ProjectPill variant="row" />\n```'),
      call('c3', 'bash', { command: 'npx tsc -p tsconfig.app.json --noEmit' }),
    ],
    stopReason: 'toolUse', errorMessage: null, timestamp: 3,
  },
  result('c3', 'bash', "src/a.ts(2,3): error TS2304: Cannot find name 'x'.", true),
  { type: 'assistant', content: text('One type error remains; fixing it now.'), stopReason: 'error', errorMessage: 'Rate limit reached. Waiting for it to lift.', timestamp: 4 },
] as unknown as AgentMessage[]

export const DEMO_TEAM = [
  { id: 'a1', name: 'Mira', role: 'Research lead', status: 'active', reportsTo: null, model: null, allowedTools: null, owns: [], color: '#3e63dd', face: null },
  { id: 'a2', name: 'Theo', role: 'Operations', status: 'active', reportsTo: 'a1', model: null, allowedTools: null, owns: [], color: '#30a46c', face: null },
] as never[]

export const DEMO_BOT_MESSAGES = [
  { id: 'm1', from: 'user', text: '@Mira can you check the overnight runs and tell me what failed?', ts: 1, kind: 'message', mentions: ['a1'] },
  {
    id: 'm2', from: 'a1', ts: 2, kind: 'message', mentions: [],
    text: 'Three of the five finished. Two failed on the same step, and I think I know why. Want me to retry them with a longer timeout?',
    steps: [
      { id: 's1', tool: 'read_file', label: 'Read the run logs', state: 'done' },
      { id: 's2', tool: 'bash', label: 'Compare the failing steps', state: 'done' },
    ],
  },
  { id: 'm3', from: 'a2', ts: 3, kind: 'routine', mentions: [], text: 'Morning check-in: inbox is clear, two invoices are waiting on you.' },
  { id: 'm4', from: 'a1', ts: 4, kind: 'needs_you', mentions: [], text: 'I would like to email the vendor about the late delivery. Approve below.' },
] as never[]

export const DEMO_APPROVALS = [
  {
    root: 'C:/demo', botId: 'a1', requestId: 'r1', toolName: 'email',
    args: { action: 'send', to: 'priya.raman@northwind.co', subject: 'Thursday’s review', text: 'Hi Priya, moving our review to 3pm. — Mira' },
    reason: 'Sending email always asks first.',
  },
] as never[]
