import { useModsStore, chatRenderer } from '../store/modsStore'
import { ModFrame } from '../host/ModFrame'

/** Whether a running mod renders this fence (Markdown asks before routing). */
export function useModFence(fence: string | undefined) {
  const mods = useModsStore((s) => s.mods)
  return fence ? chatRenderer(mods, fence) : undefined
}

/** A ```` ```fence ```` block rendered by the mod that registered the fence,
 * with the block's text as the page's `context.data` (updated live while
 * the reply streams). */
export function ModChatBlock({ fence, code }: { fence: string; code: string }) {
  const renderer = useModFence(fence)
  if (!renderer) return null
  return <ModFrame mod={renderer.mod} slot="chat" entry={renderer.entry} data={code} minHeight={40} maxHeight={900} />
}
