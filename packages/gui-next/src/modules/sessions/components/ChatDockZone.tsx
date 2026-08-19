import { useState } from 'react'
import Stack from '@mui/material/Stack'
import { useThreadsStore } from '../store/threadsStore'
import { ThreadPane } from './ThreadPane'

export const SESSION_DRAG_MIME = 'application/x-viv-session-id'

/**
 * Split-pane docking zone for secondary chat threads — opened via
 * right-click "Open in split view" on a thread row, or dragged in. Only
 * mounted (by `App.tsx`) once at least one thread is open here. Distinct
 * from the right-side Files/Preview/Terminal dock (`modules/dock`), which is
 * a different concept (workspace tools, not chat threads).
 */
export function ChatDockZone() {
  const openIds = useThreadsStore((s) => s.openIds)
  const focusedId = useThreadsStore((s) => s.focusedId)
  const open = useThreadsStore((s) => s.open)
  const [dragOver, setDragOver] = useState(false)

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)
    const id = e.dataTransfer.getData(SESSION_DRAG_MIME) || e.dataTransfer.getData('text/plain')
    if (id) open(id)
  }

  // `dropEffect` has to be set on every dragover or the browser falls back to
  // "none" and paints the no-drop cursor even though onDrop is wired.
  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    setDragOver(true)
  }

  return (
    <Stack
      direction="row"
      onDragOver={onDragOver}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
      sx={{
        flex: 1,
        minHeight: 0,
        minWidth: 0,
        gap: 1,
        p: 1,
        overflowX: 'auto',
        bgcolor: dragOver ? 'action.hover' : 'transparent',
      }}
    >
      {openIds.map((id) => (
        <ThreadPane key={id} threadId={id} focused={id === focusedId} />
      ))}
    </Stack>
  )
}
