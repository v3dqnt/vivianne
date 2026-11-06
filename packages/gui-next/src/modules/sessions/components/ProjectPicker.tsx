import { useMemo, useState } from 'react'
import { Box, ButtonBase, Dialog, InputBase, Stack, Tooltip, Typography } from '../../../ui'
import { AddLinear, CloseLinear, FolderLinear, SearchNormalLinear } from '../../../icons'
import { useProjectsStore } from '../store/projectsStore'
import { useDaysStore } from '../store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { TOOLBAR_PILL } from '../../ai/lib/toolbarPill'
import { NewProjectDialog } from './NewProjectDialog'
import { PairProjectDialog } from '../../multiplayer/components/PairProjectDialog'
import { ProjectMark } from './ProjectMark'

const ROW = {
  display: 'flex',
  width: '100%',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: '12px',
  height: 36,
  px: '12px',
  borderRadius: '8px',
  fontSize: '0.875rem',
  textAlign: 'left',
  whiteSpace: 'nowrap',
  '&:hover': { bgcolor: 'action.hover' },
} as const

/** Which project the open thread is filed under, and the way to change it:
 * a registered project, a new one, or none. */
export function ProjectPill({ variant = 'pill' }: { variant?: 'pill' | 'row' }) {
  const todayId = useDaysStore((s) => s.todayId)
  const projects = useProjectsStore((s) => s.projects)
  const projectId = useProjectsStore((s) => (todayId ? s.sessionProjectMap[todayId] : undefined))
  const busy = useAgentStore((s) => s.isBusy)
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)
  const [pairing, setPairing] = useState(false)
  const project = projects.find((p) => p.id === projectId)

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase()
    return q ? projects.filter((p) => `${p.name} ${p.path ?? ''}`.toLowerCase().includes(q)) : projects
  }, [projects, query])

  const choose = (id: string | null) => {
    setOpen(false)
    setQuery('')
    void useDaysStore.getState().assignThread(id)
  }

  return (
    <>
      <Tooltip title={busy ? 'Wait for the current turn to finish' : project ? 'Change project' : 'Add this thread to a project'}>
        <span>
          <ButtonBase
            aria-label="Project"
            disabled={busy || !todayId}
            onClick={() => setOpen(true)}
            sx={
              variant === 'row'
                ? { display: 'flex', width: '100%', justifyContent: 'flex-start', alignItems: 'center', gap: '8px', height: 32, px: '8px', borderRadius: '6px', fontSize: '0.8125rem', fontWeight: 500, color: project ? 'text.primary' : 'text.secondary', whiteSpace: 'nowrap', overflow: 'hidden', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' }, '&:disabled': { opacity: 0.5 } }
                : { ...TOOLBAR_PILL, maxWidth: 160, whiteSpace: 'nowrap' }
            }
          >
            {project ? <ProjectMark project={project} size={variant === 'row' ? 18 : 14} /> : <FolderLinear size={variant === 'row' ? 16 : 14} color="currentColor" />}
            <Box component="span" sx={{ overflow: 'hidden', textOverflow: 'ellipsis', flex: 1, textAlign: 'left' }}>
              {project?.name ?? 'Choose a project'}
            </Box>
          </ButtonBase>
        </span>
      </Tooltip>

      <Dialog open={open} onClose={() => setOpen(false)} maxWidth={false} slotProps={{ paper: { sx: { width: 'min(640px, 94vw)', maxWidth: 'none', p: '8px' } } }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 40, px: '12px', borderRadius: '8px', bgcolor: 'action.hover', color: 'text.secondary' }}>
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && matches[0]) choose(matches[0].id)
            }}
            placeholder="Search projects"
            inputProps={{ 'aria-label': 'Search projects' }}
            sx={{ flex: 1, fontSize: '0.875rem', color: 'text.primary' }}
          />
        </Stack>
        <Stack sx={{ pt: '8px', gap: '2px', maxHeight: 420, overflowY: 'auto' }}>
          <ButtonBase onClick={() => setCreating(true)} sx={{ ...ROW, fontWeight: 500 }}>
            <AddLinear size={14} color="currentColor" />
            Add project
          </ButtonBase>
          {project && (
            <ButtonBase onClick={() => choose(null)} sx={{ ...ROW, color: 'text.secondary' }}>
              <CloseLinear size={14} color="currentColor" />
              Remove from {project.name}
            </ButtonBase>
          )}
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', px: '12px', pt: '8px', pb: '4px' }}>Projects</Typography>
          {matches.length === 0 && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '12px', py: '8px' }}>No projects match.</Typography>}
          {matches.map((p) => (
            <ButtonBase key={p.id} onClick={() => choose(p.id)} aria-current={p.id === projectId ? 'true' : undefined} sx={{ ...ROW, bgcolor: p.id === projectId ? 'action.selected' : 'transparent' }}>
              <ProjectMark project={p} />
              <Box component="span" sx={{ fontWeight: 500, flexShrink: 0 }}>
                {p.name}
              </Box>
              <Box component="span" sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', fontSize: '0.8125rem', color: 'text.secondary' }}>
                {p.path}
              </Box>
            </ButtonBase>
          ))}
        </Stack>
      </Dialog>

      <NewProjectDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(p) => choose(p.id)}
        onMultiplayer={() => {
          setCreating(false)
          setPairing(true)
        }}
      />
      <PairProjectDialog open={pairing} onClose={() => setPairing(false)} />
    </>
  )
}
