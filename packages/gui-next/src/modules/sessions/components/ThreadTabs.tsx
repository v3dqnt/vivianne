import { useEffect, useState } from 'react'
import { Box, ButtonBase, CircularProgress, IconButton, Stack, Tooltip } from '../../../ui'
import { AddLinear, CloseLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { useDaysStore, holderOf } from '../store/daysStore'
import { useProjectsStore } from '../store/projectsStore'
import { useAppView } from '../../git/components/GitView'
import { agentStoreFor } from '../../ai/store/agentStore'
import type { SessionInfo } from '../lib/types'
import { titleOf } from './ThreadsSidebar'
import { ProjectMark } from './ProjectMark'

function Running({ holder }: { holder: string }) {
  const busy = agentStoreFor(holder)((s) => s.isBusy)
  return busy ? <CircularProgress size={10} sx={{ flexShrink: 0 }} /> : null
}

/** The open threads, along the top. The plus starts a blank thread. */
export function ThreadTabs() {
  const tabs = useDaysStore((s) => s.tabs)
  const openId = useDaysStore((s) => s.todayId)
  const threads = useDaysStore((s) => s.threads)
  const view = useAppView((s) => s.view)
  const map = useProjectsStore((s) => s.sessionProjectMap)
  const projects = useProjectsStore((s) => s.projects)
  const [names, setNames] = useState<Record<string, string>>({})

  useEffect(() => {
    void call<SessionInfo[]>('cmd_get_sessions').then((list) => setNames(Object.fromEntries((list ?? []).map((t) => [t.id, titleOf(t)]))))
  }, [tabs, threads])
  useEffect(() => {
    const off = on<{ id: string; name: string }>('session:renamed', ({ id, name }) => setNames((n) => ({ ...n, [id]: titleOf({ name } as SessionInfo) })))
    return () => void off.then((f) => f())
  }, [])

  const focus = (id: string) => {
    useAppView.setState({ view: 'chat' })
    if (id !== openId) void useDaysStore.getState().focusThread(id)
  }

  return (
    <Stack data-tauri-drag-region="" direction="row" sx={{ flex: 1, minWidth: 0, alignItems: 'center', gap: '2px', px: '12px', height: '100%', zIndex: 1 }}>
      <Stack direction="row" sx={{ minWidth: 0, gap: '2px', alignItems: 'center', overflow: 'hidden' }}>
        {tabs.map((id) => {
          const active = view === 'chat' && id === openId
          const name = names[id] ?? 'New chat'
          const project = projects.find((p) => p.id === map[id])
          const holder = holderOf(id)
          return (
            <Stack
              key={id}
              direction="row"
              sx={{
                position: 'relative',
                flex: '0 1 180px',
                minWidth: 72,
                height: 28,
                alignItems: 'center',
                borderRadius: '6px',
                bgcolor: active ? 'action.selected' : 'transparent',
                color: active ? 'text.primary' : 'text.secondary',
                '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover', color: 'text.primary' },
                '&:hover .tab-close': { opacity: 1 },
              }}
            >
              <ButtonBase title={name} aria-current={active ? 'page' : undefined} onClick={() => focus(id)} sx={{ flex: 1, minWidth: 0, height: '100%', justifyContent: 'flex-start', gap: '6px', pl: '8px', pr: '24px', borderRadius: '6px', fontSize: '0.75rem', fontWeight: 500 }}>
                {project && <ProjectMark project={project} size={14} />}
                {holder && <Running holder={holder} />}
                <Box component="span" sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {name}
                </Box>
              </ButtonBase>
              <IconButton className="tab-close" size="small" aria-label={`Close ${name}`} onClick={() => void useDaysStore.getState().closeTab(id)} sx={{ position: 'absolute', right: 2, width: 20, height: 20, borderRadius: '4px', opacity: active ? 1 : 0, color: 'inherit' }}>
                <CloseLinear size={12} color="currentColor" />
              </IconButton>
            </Stack>
          )
        })}
      </Stack>
      <Tooltip title="New thread">
        <IconButton
          size="small"
          aria-label="New thread"
          onClick={() => {
            useAppView.setState({ view: 'chat' })
            void useDaysStore.getState().newBlankThread()
          }}
          sx={{ flexShrink: 0, width: 28, height: 28, borderRadius: '6px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
        >
          <AddLinear size={14} color="currentColor" />
        </IconButton>
      </Tooltip>
    </Stack>
  )
}
