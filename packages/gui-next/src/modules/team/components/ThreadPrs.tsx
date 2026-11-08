import { useEffect, useState } from 'react'
import { ButtonBase } from '../../../ui'
import { call, on } from '../../../ipc'
import { activeThreadId } from '../../ai/store/activeThread'
import { useDaysStore } from '../../sessions/store/daysStore'
import { usePanelStore } from '../../dock/store/panelStore'

interface PrLink {
  number: number
  url: string
  branch: string
}

/** The pull requests the open thread opened, one row each. */
export function ThreadPrs() {
  const openId = useDaysStore((s) => s.todayId)
  const [prs, setPrs] = useState<PrLink[]>([])
  useEffect(() => {
    const read = () => void call<PrLink[]>('cmd_thread_prs', { threadId: activeThreadId() }).then((p) => setPrs(p ?? [])).catch(() => setPrs([]))
    read()
    const off = on('orchestration:changed', read)
    return () => void off.then((f) => f())
  }, [openId])
  return (
    <>
      {prs.map((p) => (
        <ButtonBase
          key={p.number}
          onClick={() => usePanelStore.getState().openUrl(p.url)}
          title={p.branch}
          sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', alignItems: 'center', height: 28, px: '8px', borderRadius: '6px', fontSize: '0.8125rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
        >
          Pull request #{p.number}
        </ButtonBase>
      ))}
    </>
  )
}
