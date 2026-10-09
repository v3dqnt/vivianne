import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { DiagramKind } from '../components/sci/check'

/**
 * Answers the agent's `check_diagram` tool. The Rust side emits
 * `diagram:check` to this window; the script is run through the same parsers
 * and RDKit calls the chat's science blocks use, and the verdict goes back
 * through `cmd_diagram_check_done`. The checking code loads only when a
 * request arrives, so a session that never draws a diagram never pays for it.
 */

interface CheckRequest {
  id: string
  kind: DiagramKind
  code: string
}

async function verdict(kind: DiagramKind, code: string): Promise<string> {
  try {
    const { checkScript } = await import('../components/sci/check')
    const rd = kind === 'chem' ? await (await import('../components/sci/rdkit')).loadRDKit() : undefined
    return await checkScript(kind, code, rd)
  } catch (e) {
    return `Could not check: ${e instanceof Error ? e.message : String(e)}`
  }
}

/** Start listening. Called once, from the main window. */
export function initDiagramCheck() {
  void listen<CheckRequest>('diagram:check', async (event) => {
    const { id, kind, code } = event.payload
    const message = await verdict(kind, code)
    await invoke('cmd_diagram_check_done', { id, message }).catch(() => {})
  })
}
