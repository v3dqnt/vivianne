import { useEffect, useMemo, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import Tooltip from '@mui/material/Tooltip'
import { MessageAddLinear, SearchNormalLinear, CloseCircleLinear, SettingsLinear } from 'mx-icons'
import { useSessionsStore } from '../store/sessionsStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { groupByWorkspace } from '../lib/groupByWorkspace'
import { SessionRow } from './SessionRow'

export const SIDEBAR_WIDTH = 260

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const sessions = useSessionsStore((s) => s.sessions)
  const activeSessionId = useSessionsStore((s) => s.activeSessionId)
  const refresh = useSessionsStore((s) => s.refresh)
  const refreshActiveId = useSessionsStore((s) => s.refreshActiveId)
  const newChat = useSessionsStore((s) => s.newChat)
  const isBusy = useAgentStore((s) => s.isBusy)

  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    void refresh()
    void refreshActiveId()
  }, [refresh, refreshActiveId])

  // A turn just finished (auto-saved server-side on every turn) — the
  // sidebar's list is otherwise stale until this, most visibly for a brand
  // new thread that didn't exist as a session until its first reply landed.
  const wasBusy = useRef(false)
  useEffect(() => {
    if (wasBusy.current && !isBusy) void refresh()
    wasBusy.current = isBusy
  }, [isBusy, refresh])

  useEffect(() => {
    if (searchOpen) searchRef.current?.focus()
  }, [searchOpen])

  const filtered = useMemo(() => {
    if (!query.trim()) return sessions
    const q = query.trim().toLowerCase()
    return sessions.filter((s) => s.name.toLowerCase().includes(q))
  }, [sessions, query])

  const groups = useMemo(() => groupByWorkspace(filtered), [filtered])

  return (
    <Stack
      component="nav"
      sx={{
        width: SIDEBAR_WIDTH,
        flexShrink: 0,
        height: '100%',
        borderRight: 1,
        borderColor: 'divider',
        bgcolor: 'background.default',
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', px: 1, py: 1, gap: 0.5, flexShrink: 0 }}>
        {searchOpen ? (
          <Stack direction="row" sx={{ alignItems: 'center', flex: 1, px: 1 }}>
            <Box sx={{ display: 'flex', color: 'text.secondary', mr: 1 }}>
              <SearchNormalLinear size={16} color="currentColor" />
            </Box>
            <InputBase
              inputRef={searchRef}
              fullWidth
              placeholder="Search threads…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setQuery('')
                  setSearchOpen(false)
                }
              }}
              sx={{ fontSize: '0.8125rem' }}
            />
            <IconButton
              size="small"
              onClick={() => {
                setQuery('')
                setSearchOpen(false)
              }}
            >
              <CloseCircleLinear size={15} color="currentColor" />
            </IconButton>
          </Stack>
        ) : (
          <>
            <Tooltip title="New chat">
              <IconButton size="small" onClick={() => void newChat()} sx={{ flex: 1, borderRadius: 1.5, gap: 1, justifyContent: 'flex-start', px: 1.5 }}>
                <MessageAddLinear size={17} color="currentColor" />
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  New chat
                </Typography>
              </IconButton>
            </Tooltip>
            <Tooltip title="Search threads">
              <IconButton size="small" onClick={() => setSearchOpen(true)}>
                <SearchNormalLinear size={17} color="currentColor" />
              </IconButton>
            </Tooltip>
          </>
        )}
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', py: 0.5 }}>
        {groups.length === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 2, py: 2 }}>
            {query ? 'No matching threads.' : 'No past chats.'}
          </Typography>
        )}
        {groups.map((group) => (
          <Box key={group.path ?? '__none__'} sx={{ mb: 1 }}>
            <Typography
              variant="caption"
              color="text.secondary"
              title={group.path ?? undefined}
              sx={{ display: 'block', px: 2, py: 0.5, fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.65rem' }}
            >
              {group.label}
            </Typography>
            {group.sessions.map((session) => (
              <SessionRow key={session.id} session={session} active={session.id === activeSessionId} />
            ))}
          </Box>
        ))}
      </Box>

      <Box sx={{ borderTop: 1, borderColor: 'divider', p: 1, flexShrink: 0 }}>
        <IconButton
          size="small"
          onClick={onOpenSettings}
          sx={{ width: '100%', borderRadius: 1.5, gap: 1, justifyContent: 'flex-start', px: 1.5 }}
        >
          <SettingsLinear size={17} color="currentColor" />
          <Typography variant="body2">Settings</Typography>
        </IconButton>
      </Box>
    </Stack>
  )
}
