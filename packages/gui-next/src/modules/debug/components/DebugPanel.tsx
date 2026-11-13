import { useState } from 'react'
import { Box, ButtonBase, IconButton, Stack, Typography } from '../../../ui'
import {
  ArrowDownLinear, ArrowRightLinear, ArrowUpLinear, ChevronRightLinear, PlayLinear, RefreshLinear, StopLinear,
} from '../../../icons'
import { SegmentedTabs, type SegmentedTab } from '../../../components/SegmentedTabs'

export type DebugStatus = 'starting' | 'running' | 'stopped' | 'exited'

export interface DebugFrame { id: number; name: string; file: string; line: number }
export interface DebugVariable { name: string; value: string; type?: string; children?: DebugVariable[] }
export interface DebugBreakpoint { file: string; line: number; condition?: string; verified: boolean; kind?: 'line' | 'function' | 'exception' | 'data' }
export interface DebugSession {
  adapter: string
  program: string
  status: DebugStatus
  /** Why it stopped ("breakpoint", "exception"), or how it ended. */
  reason?: string
  exitCode?: number
  frames: DebugFrame[]
  selectedFrame: number
  scopes: { name: string; variables: DebugVariable[] }[]
  breakpoints: DebugBreakpoint[]
  output: string
  /** An exception the program stopped on. */
  exception?: { type: string; message: string }
}

type Section = 'stack' | 'variables' | 'breakpoints' | 'output'

const STATUS: Record<DebugStatus, { label: string; color: string }> = {
  starting: { label: 'Starting', color: 'warning.main' },
  running: { label: 'Running', color: 'success.main' },
  stopped: { label: 'Paused', color: 'warning.main' },
  exited: { label: 'Ended', color: 'text.secondary' },
}

const mono = { fontFamily: 'var(--font-mono, ui-monospace, monospace)', fontSize: '0.75rem' } as const

function Dot({ color }: { color: string }) {
  return <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: color, flexShrink: 0 }} />
}

function Controls({ session, on }: { session: DebugSession; on: DebugHandlers }) {
  const live = session.status !== 'exited'
  const paused = session.status === 'stopped'
  const btn = (label: string, icon: React.ReactNode, act: () => void, enabled: boolean) => (
    <IconButton size="small" aria-label={label} title={label} disabled={!enabled} onClick={act} sx={{ borderRadius: '8px' }}>{icon}</IconButton>
  )
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: '2px' }}>
      {paused
        ? btn('Continue', <PlayLinear size={16} color="currentColor" />, on.onContinue, true)
        : (
          <ButtonBase disabled={session.status !== 'running'} onClick={on.onPause} sx={{ height: 28, px: '10px', borderRadius: '8px', fontSize: '0.75rem', fontWeight: 500, '&:hover': { bgcolor: 'action.hover' } }}>
            Pause
          </ButtonBase>
        )}
      {btn('Step over', <ArrowRightLinear size={16} color="currentColor" />, on.onStepOver, paused)}
      {btn('Step into', <ArrowDownLinear size={16} color="currentColor" />, on.onStepIn, paused)}
      {btn('Step out', <ArrowUpLinear size={16} color="currentColor" />, on.onStepOut, paused)}
      {btn('Restart', <RefreshLinear size={16} color="currentColor" />, on.onRestart, live)}
      {btn('Stop', <StopLinear size={16} color="currentColor" />, on.onStop, live)}
    </Stack>
  )
}

function Row({ children, active, onClick, indent = 0 }: { children: React.ReactNode; active?: boolean; onClick?: () => void; indent?: number }) {
  return (
    <ButtonBase
      onClick={onClick}
      disabled={!onClick}
      sx={{ width: '100%', justifyContent: 'flex-start', textAlign: 'left', gap: '8px', py: '5px', pl: `${12 + indent * 14}px`, pr: '12px', borderRadius: '6px', bgcolor: active ? 'action.selected' : 'transparent', '&:hover': { bgcolor: onClick ? 'action.hover' : undefined }, '&.Mui-disabled': { opacity: 1 } }}
    >
      {children}
    </ButtonBase>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return <Typography sx={{ px: '12px', py: '16px', fontSize: '0.8125rem', color: 'text.secondary' }}>{children}</Typography>
}

function Stack_({ session, on }: { session: DebugSession; on: DebugHandlers }) {
  if (session.status !== 'stopped') return <Empty>{session.status === 'running' ? 'The program is running. Pause it, or wait for a breakpoint.' : 'Nothing to show.'}</Empty>
  return (
    <Stack sx={{ gap: '1px', p: '4px' }}>
      {session.frames.map((f) => (
        <Row key={f.id} active={f.id === session.selectedFrame} onClick={() => on.onSelectFrame(f.id)}>
          <Typography sx={{ ...mono, fontWeight: 500, flexShrink: 0 }}>{f.name}</Typography>
          <Typography noWrap sx={{ ...mono, color: 'text.secondary', minWidth: 0 }}>{f.file.split(/[\\/]/).pop()}:{f.line}</Typography>
        </Row>
      ))}
    </Stack>
  )
}

function VariableRow({ v, depth, open, toggle }: { v: DebugVariable; depth: number; open: Set<string>; toggle: (k: string) => void }) {
  const key = `${depth}:${v.name}`
  const expandable = !!v.children?.length
  const isOpen = open.has(key)
  return (
    <>
      <Row indent={depth} onClick={expandable ? () => toggle(key) : undefined}>
        <Box sx={{ width: 12, display: 'flex', flexShrink: 0, color: 'text.secondary', transform: isOpen ? 'rotate(90deg)' : 'none', transition: 'transform 120ms' }}>
          {expandable && <ChevronRightLinear size={12} color="currentColor" />}
        </Box>
        <Typography sx={{ ...mono, color: 'primary.main', flexShrink: 0 }}>{v.name}</Typography>
        <Typography noWrap sx={{ ...mono, minWidth: 0, flex: 1 }}>{v.value}</Typography>
        {v.type && <Typography sx={{ ...mono, color: 'text.secondary', flexShrink: 0, fontSize: '0.6875rem' }}>{v.type}</Typography>}
      </Row>
      {isOpen && v.children?.map((c) => <VariableRow key={c.name} v={c} depth={depth + 1} open={open} toggle={toggle} />)}
    </>
  )
}

function Variables({ session }: { session: DebugSession }) {
  const [open, setOpen] = useState<Set<string>>(new Set())
  const toggle = (k: string) => setOpen((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n })
  if (session.status !== 'stopped') return <Empty>Variables show while the program is paused.</Empty>
  return (
    <Stack sx={{ p: '4px', gap: '6px' }}>
      {session.scopes.map((s) => (
        <Box key={s.name}>
          <Typography sx={{ px: '12px', pt: '4px', pb: '2px', fontSize: '0.6875rem', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'text.secondary' }}>{s.name}</Typography>
          {s.variables.map((v) => <VariableRow key={v.name} v={v} depth={0} open={open} toggle={toggle} />)}
        </Box>
      ))}
    </Stack>
  )
}

function Breakpoints({ session, on }: { session: DebugSession; on: DebugHandlers }) {
  if (!session.breakpoints.length) return <Empty>No breakpoints. Ask the agent to set one, or add it from a file.</Empty>
  return (
    <Stack sx={{ gap: '1px', p: '4px' }}>
      {session.breakpoints.map((b) => (
        <Row key={`${b.file}:${b.line}`}>
          <Dot color={b.verified ? 'error.main' : 'text.disabled'} />
          <Typography noWrap sx={{ ...mono, minWidth: 0, flex: 1 }}>
            {b.file.split(/[\\/]/).pop()}:{b.line}
            {b.condition && <Box component="span" sx={{ color: 'text.secondary' }}>  if {b.condition}</Box>}
          </Typography>
          {!b.verified && <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary' }}>not bound</Typography>}
          <ButtonBase onClick={() => on.onRemoveBreakpoint(b)} sx={{ fontSize: '0.75rem', color: 'text.secondary', px: '6px', borderRadius: '6px', '&:hover': { color: 'text.primary', bgcolor: 'action.hover' } }}>Remove</ButtonBase>
        </Row>
      ))}
    </Stack>
  )
}

function Output({ session }: { session: DebugSession }) {
  if (!session.output) return <Empty>The program hasn't printed anything.</Empty>
  return <Box component="pre" sx={{ ...mono, m: 0, p: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: 'text.primary' }}>{session.output}</Box>
}

export interface DebugHandlers {
  onContinue: () => void
  onPause: () => void
  onStepOver: () => void
  onStepIn: () => void
  onStepOut: () => void
  onRestart: () => void
  onStop: () => void
  onSelectFrame: (id: number) => void
  onRemoveBreakpoint: (b: DebugBreakpoint) => void
}

/** The program being debugged: where it is, what it holds, where it will stop. Draws a session it is given; the app feeds it from the backend. */
export function DebugPanel({ session, ...on }: { session: DebugSession | null } & DebugHandlers) {
  const [section, setSection] = useState<Section>('stack')
  if (!session) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: '6px', p: '24px', textAlign: 'center' }}>
        <Typography sx={{ fontSize: '0.875rem', fontWeight: 500 }}>Nothing is being debugged</Typography>
        <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', maxWidth: 280 }}>When the agent runs a program under a debugger, its stack, variables and breakpoints show here.</Typography>
      </Stack>
    )
  }
  const st = STATUS[session.status]
  const tabs: SegmentedTab<Section>[] = [
    { id: 'stack', label: 'Stack' },
    { id: 'variables', label: 'Variables' },
    { id: 'breakpoints', label: `Breaks${session.breakpoints.length ? ` ${session.breakpoints.length}` : ''}` },
    { id: 'output', label: 'Output' },
  ]
  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack sx={{ gap: '6px', p: '12px', borderBottom: 1, borderColor: 'divider' }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: '8px' }}>
          <Dot color={st.color} />
          <Typography sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>{st.label}</Typography>
          {session.reason && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{session.reason}{session.status === 'exited' && session.exitCode !== undefined ? `, code ${session.exitCode}` : ''}</Typography>}
          <Box sx={{ flex: 1 }} />
          <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary' }}>{session.adapter}</Typography>
        </Stack>
        <Typography noWrap sx={{ ...mono, color: 'text.secondary' }}>{session.program}</Typography>
        <Controls session={session} on={on} />
        {session.exception && (
          <Box sx={{ mt: '2px', px: '10px', py: '8px', borderRadius: '8px', bgcolor: 'action.hover', borderLeft: 3, borderColor: 'error.main' }}>
            <Typography sx={{ ...mono, fontWeight: 600 }}>{session.exception.type}</Typography>
            <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', wordBreak: 'break-word' }}>{session.exception.message}</Typography>
          </Box>
        )}
      </Stack>
      <Box sx={{ p: '8px 12px 0' }}>
        <SegmentedTabs fill value={section} onChange={setSection} tabs={tabs} />
      </Box>
      <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', pt: '4px' }}>
        {section === 'stack' && <Stack_ session={session} on={on} />}
        {section === 'variables' && <Variables session={session} />}
        {section === 'breakpoints' && <Breakpoints session={session} on={on} />}
        {section === 'output' && <Output session={session} />}
      </Box>
    </Stack>
  )
}
