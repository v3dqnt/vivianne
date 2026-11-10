import { create } from 'zustand'
import { Button, Dialog, DialogActions, DialogContent, DialogTitle, Typography } from '../ui'
import { toast } from '../modules/mods/store/toastStore'

interface Ask {
  message: string
  /** The confirm button's word: what happens, not "OK". */
  action: string
  danger: boolean
  resolve: (ok: boolean) => void
}

const useConfirmStore = create<{ ask: Ask | null }>(() => ({ ask: null }))

/** Ask before something that can't be taken back. Resolves `true` only when
 * the user pressed the action button. A second question while one is open
 * answers the first with "no". */
export function confirm(message: string, opts: { action?: string; danger?: boolean } = {}): Promise<boolean> {
  return new Promise((resolve) => {
    useConfirmStore.getState().ask?.resolve(false)
    useConfirmStore.setState({ ask: { message, action: opts.action ?? 'Delete', danger: opts.danger ?? true, resolve } })
  })
}

/** Run something the user asked for and say so when it fails, instead of
 * leaving the screen looking as if nothing happened. */
export async function orToast<T>(what: string, run: () => Promise<T>): Promise<T | undefined> {
  try {
    return await run()
  } catch (e) {
    toast(`${what}: ${e instanceof Error ? e.message : String(e)}`)
    return undefined
  }
}

/** Mounted once, at the app root. */
export function ConfirmHost() {
  const ask = useConfirmStore((s) => s.ask)
  if (!ask) return null
  const answer = (ok: boolean) => {
    useConfirmStore.setState({ ask: null })
    ask.resolve(ok)
  }
  return (
    <Dialog open onClose={() => answer(false)} maxWidth="xs" fullWidth>
      <DialogTitle>{ask.danger ? 'Are you sure?' : 'Confirm'}</DialogTitle>
      <DialogContent>
        <Typography variant="body2">{ask.message}</Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => answer(false)} color="inherit" autoFocus>
          Cancel
        </Button>
        <Button onClick={() => answer(true)} variant="contained" color={ask.danger ? 'error' : 'primary'}>
          {ask.action}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
