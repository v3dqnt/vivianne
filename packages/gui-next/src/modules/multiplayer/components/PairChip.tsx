import { useEffect } from 'react'
import { Box, ButtonBase, Tooltip, alpha, useTheme } from '../../../ui'
import { useMultiplayerStore } from '../store/multiplayerStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'
import { usePanelStore } from '../../dock/store/panelStore'

/**
 * In the title bar of a multiplayer project: your partner, whether they're
 * online (and whether their agent is working), and unread messages. Opens
 * the Multiplayer panel.
 */
export function PairChip() {
  const theme = useTheme()
  const project = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId))
  const status = useMultiplayerStore((s) => s.status)
  const unread = useMultiplayerStore((s) => s.unread)
  const activeAt = useMultiplayerStore((s) => s.partnerActiveAt)
  const init = useMultiplayerStore((s) => s.init)
  useEffect(() => {
    if (project?.multiplayer) void init()
  }, [project?.multiplayer, init])

  if (!project?.multiplayer) return null
  const name = project.multiplayer.peer_display_name || 'Partner'
  const live = status.state === 'connected' && status.peerId === project.multiplayer.peer_id
  const working = live && Date.now() - activeAt < 30_000
  const initials = name
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase())
    .join('')
  const color = theme.palette.success.main
  const tip = live ? (working ? `${name}'s agent is working` : `${name} is online`) : `${name} is offline`

  return (
    <Tooltip title={`${tip}${unread ? ` · ${unread} unread` : ''}`}>
      <ButtonBase
        onClick={() => usePanelStore.getState().openRight('multiplayer')}
        sx={{ position: 'relative', gap: 0.75, height: 28, pl: 0.5, pr: 1, mr: 0.5, borderRadius: '999px', '&:hover': { bgcolor: 'action.hover' } }}
      >
        <Box sx={{ position: 'relative' }}>
          <Box sx={{ width: 20, height: 20, borderRadius: '50%', display: 'grid', placeItems: 'center', fontSize: '0.5625rem', fontWeight: 700, bgcolor: alpha(color, 0.16), color }}>
            {initials || '?'}
          </Box>
          <Box
            sx={{
              position: 'absolute',
              right: -2,
              bottom: -2,
              width: 8,
              height: 8,
              borderRadius: '50%',
              border: 2,
              borderColor: 'background.chrome',
              bgcolor: live ? 'success.main' : 'text.disabled',
            }}
          />
        </Box>
        <Box component="span" sx={{ fontSize: '0.75rem', fontWeight: 600, color: live ? 'text.primary' : 'text.secondary', maxWidth: 110, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {name}
        </Box>
        {unread > 0 && (
          <Box sx={{ minWidth: 16, height: 16, px: 0.5, borderRadius: '8px', bgcolor: 'primary.main', color: 'primary.contrastText', fontSize: '0.625rem', fontWeight: 700, display: 'grid', placeItems: 'center' }}>
            {unread}
          </Box>
        )}
      </ButtonBase>
    </Tooltip>
  )
}
