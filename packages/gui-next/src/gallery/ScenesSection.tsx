import { useEffect, useState, type ReactNode } from 'react'
import { Box, Stack, Typography } from '../ui'
import { useProjectsStore } from '../modules/sessions/store/projectsStore'
import { useDaysStore } from '../modules/sessions/store/daysStore'
import { useGitStatus } from '../modules/git/store/gitStatusStore'
import { useModelStore } from '../modules/ai/store/modelStore'
import { useAgentStore } from '../modules/ai/store/agentStore'
import { useWorkspaceStore } from '../modules/ai/store/workspaceStore'
import { ThreadTabs } from '../modules/sessions/components/ThreadTabs'
import { ProjectTools, RAIL_VARS } from '../modules/team/components/ProjectTools'
import { ChatView } from '../modules/ai/components/ChatView'
import { WidgetLab } from '../modules/team/components/WidgetLab'
import { BotChat } from '../modules/team/components/BotChat'
import { DebugPanel, type DebugSession } from '../modules/debug/components/DebugPanel'
import { Boundary } from './Boundary'
import { DEMO_MESSAGES, DEMO_TEAM, DEMO_BOT_MESSAGES, DEMO_APPROVALS } from './demoData'

const ROOT = 'C:/demo'

const DEBUG_STOPPED: DebugSession = {
  adapter: 'debugpy', program: 'C:/demo/app/billing.py', status: 'stopped', reason: 'breakpoint', selectedFrame: 1,
  frames: [
    { id: 1, name: 'apply_discount', file: 'C:/demo/app/billing.py', line: 42 },
    { id: 2, name: 'total', file: 'C:/demo/app/billing.py', line: 77 },
    { id: 3, name: 'checkout', file: 'C:/demo/app/cart.py', line: 18 },
    { id: 4, name: '<module>', file: 'C:/demo/app/main.py', line: 9 },
  ],
  scopes: [
    { name: 'Locals', variables: [
      { name: 'price', value: '49.0', type: 'float' },
      { name: 'rate', value: '0.15', type: 'float' },
      { name: 'items', value: 'list (3)', type: 'list', children: [
        { name: '0', value: "{'sku': 'A1', 'qty': 2}", type: 'dict' },
        { name: '1', value: "{'sku': 'B7', 'qty': 1}", type: 'dict' },
        { name: '2', value: "{'sku': 'C3', 'qty': 4}", type: 'dict' },
      ] },
    ] },
    { name: 'Globals', variables: [{ name: 'TAX', value: '0.2', type: 'float' }] },
  ],
  breakpoints: [
    { file: 'C:/demo/app/billing.py', line: 42, verified: true },
    { file: 'C:/demo/app/billing.py', line: 77, condition: 'total > 100', verified: true },
    { file: 'C:/demo/app/cart.py', line: 31, verified: false },
  ],
  output: 'Loading cart for user 14\nApplying code SPRING15\n',
}
const DEBUG_CRASH: DebugSession = {
  ...DEBUG_STOPPED, reason: 'exception',
  exception: { type: 'ZeroDivisionError', message: 'division by zero, in apply_discount at billing.py:42' },
}
const DEBUG_RUNNING: DebugSession = { ...DEBUG_STOPPED, status: 'running', reason: undefined, frames: [], scopes: [] }
const DEBUG_ENDED: DebugSession = { ...DEBUG_STOPPED, status: 'exited', reason: 'exited', exitCode: 1, frames: [], scopes: [], output: 'Traceback (most recent call last):\n  File "billing.py", line 42\nZeroDivisionError: division by zero\n' }
const NOOP = { onContinue() {}, onPause() {}, onStepOver() {}, onStepIn() {}, onStepOut() {}, onRestart() {}, onStop() {}, onSelectFrame() {}, onRemoveBreakpoint() {} }

/** Fill the app's stores with a believable session, so whole screens draw as they do in the app. */
function seed() {
  useProjectsStore.setState({
    projects: [{ id: 'p1', name: 'Project Versa', created_at: '2026-10-01', path: ROOT, color: '#3e63dd', multiplayer: null }],
    sessionProjectMap: { t1: 'p1', t2: 'p1' },
  })
  useDaysStore.setState({ tabs: ['t1', 't2', 't3'], todayId: 't1' })
  useWorkspaceStore.setState({ folders: [ROOT] })
  useGitStatus.setState({
    root: ROOT,
    repo: 'v3dqnt/vivianne',
    summary: { branch: 'agent/threads', upstream: 'origin/agent/threads', ahead: 2, behind: 0, staged: 0, unstaged: 3, untracked: 1, conflicted: 0, defaultBranch: 'main', lastCommit: 'Add the rail' },
    pr: { number: 4, title: 'Threads on their own branches', state: 'open', draft: false, url: '#', additions: 4210, deletions: 1820, checks: 'running', failing: [] },
  })
  useModelStore.setState({
    stats: {
      contextTokens: 55100, contextWindow: 128000, contextPercent: 43, messageCount: 24, inputTokens: 45400, outputTokens: 9600, cacheRead: 30000, cacheWrite: 0,
      totalTokens: 55000, modelId: 'demo', modelName: 'Demo model', provider: 'demo', maxTokens: 8192,
      breakdown: { systemPrompt: 7200, toolSchemas: 19300, toolResults: 43000, assistant: 9600, user: 2400 },
    },
    sessionCost: { cost: 0.04 },
  } as never)
  useAgentStore.setState({ messages: DEMO_MESSAGES, isBusy: false } as never)
}

function Scene({ title, note, children, height = 560 }: { title: string; note?: string; children: ReactNode; height?: number }) {
  return (
    <Stack sx={{ gap: '8px' }}>
      <Box>
        <Typography sx={{ fontSize: '0.875rem', fontWeight: 500 }}>{title}</Typography>
        {note && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{note}</Typography>}
      </Box>
      <Box sx={{ position: 'relative', height, border: 1, borderColor: 'divider', borderRadius: '8px', overflow: 'hidden', bgcolor: 'background.default', display: 'flex', flexDirection: 'column' }}>
        <Boundary>{children}</Boundary>
      </Box>
    </Stack>
  )
}

/** Whole screens, drawn from the app's real components over seeded stores. */
export function ScenesSection() {
  const [ready, setReady] = useState(false)
  useEffect(() => {
    seed()
    setReady(true)
    // The stores refresh themselves from the backend, which a browser doesn't have, and an empty answer would wipe the demo. Put it back.
    const t = window.setInterval(seed, 700)
    return () => window.clearInterval(t)
  }, [])
  if (!ready) return null
  return (
    <Stack sx={{ gap: '24px' }}>
      <Scene title="Thread tabs" note="The tab bar along the top of the window." height={56}>
        <Box sx={{ display: 'flex', height: 40, alignItems: 'center' }}><ThreadTabs /></Box>
      </Scene>

      <Scene title="Project rail, narrow window" note="Sections stacked with dividers." height={640}>
        <Box sx={{ ...RAIL_VARS, '--rail-w': '240px', position: 'relative', flex: 1 }}><ProjectTools /></Box>
      </Scene>

      <Scene title="Project rail, large window" note="Project, goal and version control side by side with context." height={420}>
        <Box sx={{ ...RAIL_VARS, '--rail-w': '620px', position: 'relative', flex: 1 }}><ProjectTools /></Box>
      </Scene>

      <Scene title="Debugger: paused at a breakpoint" note="Stack, variables, breakpoints and output of a program the agent is debugging." height={520}>
        <Box sx={{ width: 380, flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}><DebugPanel session={DEBUG_STOPPED} {...NOOP} /></Box>
      </Scene>

      <Scene title="Debugger: stopped on an exception" height={520}>
        <Box sx={{ width: 380, flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}><DebugPanel session={DEBUG_CRASH} {...NOOP} /></Box>
      </Scene>

      <Scene title="Debugger: running, ended, and nothing started" note="The three quiet states." height={420}>
        <Box sx={{ display: 'flex', flex: 1, minHeight: 0, gap: '1px', bgcolor: 'divider' }}>
          {[DEBUG_RUNNING, DEBUG_ENDED, null].map((s, i) => (
            <Box key={i} sx={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', bgcolor: 'background.default' }}><DebugPanel session={s} {...NOOP} /></Box>
          ))}
        </Box>
      </Scene>

      <Scene title="Main chat" note="A conversation with tool calls and results, a failed step, and the composer." height={720}>
        <ChatView />
      </Scene>

      <Scene title="24/7 agents: group chat" note="A teammate's replies with steps, a routine report, and an approval card waiting on you." height={720}>
        <BotChat
          root={ROOT}
          channel="room"
          messages={DEMO_BOT_MESSAGES}
          team={DEMO_TEAM}
          approvals={DEMO_APPROVALS}
          onSend={async () => {}}
          placeholder="Message the team"
          allowEveryone
        />
      </Scene>

      <Scene title="24/7 agents: approval widgets" note="Every approval card, in every state." height={720}>
        <WidgetLab />
      </Scene>
    </Stack>
  )
}
