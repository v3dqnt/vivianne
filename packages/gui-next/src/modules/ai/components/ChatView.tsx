import { useEffect } from 'react'
import Stack from '@mui/material/Stack'
import { useAgentStore } from '../store/agentStore'
import { MessageList } from './MessageList'
import { Composer } from './Composer'

export function ChatView() {
  const init = useAgentStore((s) => s.init)

  useEffect(() => {
    void init()
  }, [init])

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <MessageList />
      <Composer />
    </Stack>
  )
}
