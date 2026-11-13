import { useDaysStore } from '../sessions/store/daysStore'
import { DebugPanel } from './components/DebugPanel'
import { useDebug } from './debugStore'

/** The rail's Debug tab: the thread's debug session, with its buttons wired to the backend. */
export function DebugTab() {
  const thread = useDaysStore((s) => s.todayId)
  const session = useDebug((s) => s.session)
  const { control, selectFrame } = useDebug.getState()
  const act = (action: string) => () => thread && void control(thread, action)
  return (
    <DebugPanel
      session={thread ? session : null}
      onContinue={act('continue')}
      onPause={act('pause')}
      onStepOver={act('step_over')}
      onStepIn={act('step_in')}
      onStepOut={act('step_out')}
      onRestart={act('restart')}
      onStop={act('stop')}
      onSelectFrame={(id) => thread && selectFrame(thread, id)}
      onRemoveBreakpoint={(b) => thread && void control(thread, 'remove_breakpoint', { file: b.file, line: b.line })}
    />
  )
}
