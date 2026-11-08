import { Box } from '../../../ui'
import { MessageSquareLinear } from '../../../icons'
import { projectColor, projectInitial, GENERAL_PROJECT_ID, type Project } from '../store/projectsStore'

/** A project's square: its picture, or its initial on its colour. */
export function ProjectMark({ project, size = 20 }: { project: Pick<Project, 'id' | 'name' | 'color'> & { icon?: string | null }; size?: number }) {
  const color = projectColor(project)
  if (project.id === GENERAL_PROJECT_ID) {
    return (
      <Box sx={{ width: size, height: size, borderRadius: '6px', display: 'grid', placeItems: 'center', bgcolor: 'action.selected', color: 'text.secondary', flexShrink: 0 }}>
        <MessageSquareLinear size={size * 0.6} color="currentColor" />
      </Box>
    )
  }
  if (project.icon) return <Box component="img" src={project.icon} alt="" sx={{ width: size, height: size, borderRadius: '6px', objectFit: 'cover', flexShrink: 0 }} />
  return (
    <Box sx={{ width: size, height: size, borderRadius: '6px', display: 'grid', placeItems: 'center', bgcolor: `${color}26`, color, fontSize: size * 0.5, fontWeight: 600, flexShrink: 0 }}>
      {projectInitial(project.name)}
    </Box>
  )
}
