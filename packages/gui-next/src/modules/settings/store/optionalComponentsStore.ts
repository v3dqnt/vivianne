import { create } from 'zustand'
import { call, on } from '../../../ipc'

interface ComponentProgress {
  percent: number | null
  message: string
  error: string | null
  downloading: boolean
}

interface DockerStatus {
  installed: boolean
  running: boolean
  supportedOs: boolean
}

interface StatusResponse {
  camoufoxInstalled: boolean
  docker: DockerStatus
}

interface ProgressEvent {
  component: 'camoufox' | 'docker'
  percent: number | null
  message: string
  error: boolean
}

const EMPTY_PROGRESS: ComponentProgress = { percent: null, message: '', error: null, downloading: false }

interface OptionalComponentsState {
  initialized: boolean
  /** Whether the first-launch prompt has already been shown and answered
   * (persisted in settings.json's `optionalComponentsPrompted` — either
   * "download now" or "later" counts as answered, so it only asks once). */
  prompted: boolean
  camoufoxInstalled: boolean
  docker: DockerStatus | null
  camoufox: ComponentProgress
  dockerProgress: ComponentProgress
  init: () => Promise<void>
  refreshStatus: () => Promise<void>
  /** Marks the prompt as answered without starting anything ("Later"). */
  dismissPrompt: () => Promise<void>
  /** Marks the prompt as answered and kicks off both downloads in the
   * background — does not block on either finishing. */
  startDownload: () => Promise<void>
}

async function readSettings(): Promise<Record<string, unknown>> {
  return (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
}

async function markPrompted() {
  const settings = await readSettings()
  await call('cmd_save_settings', { settings: { ...settings, optionalComponentsPrompted: true } })
}

export const useOptionalComponentsStore = create<OptionalComponentsState>((set, get) => ({
  initialized: false,
  prompted: true, // optimistic default so a slow settings read never flashes the modal
  camoufoxInstalled: false,
  docker: null,
  camoufox: EMPTY_PROGRESS,
  dockerProgress: EMPTY_PROGRESS,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true })

    const settings = await readSettings()
    set({ prompted: settings.optionalComponentsPrompted === true })
    await get().refreshStatus()

    await on<ProgressEvent>('optional-component-progress', (p) => {
      const key = p.component === 'camoufox' ? 'camoufox' : 'dockerProgress'
      set({
        [key]: {
          percent: p.percent,
          message: p.message,
          error: p.error ? p.message : null,
          downloading: !p.error && p.percent !== 100,
        },
      } as Partial<OptionalComponentsState>)
      if (p.percent === 100 && !p.error) void get().refreshStatus()
    })
  },

  refreshStatus: async () => {
    const status = await call<StatusResponse>('cmd_get_optional_components_status')
    if (!status) return
    set({ camoufoxInstalled: status.camoufoxInstalled, docker: status.docker })
  },

  dismissPrompt: async () => {
    set({ prompted: true })
    await markPrompted()
  },

  startDownload: async () => {
    set({
      prompted: true,
      camoufox: { ...EMPTY_PROGRESS, downloading: true },
      dockerProgress: { ...EMPTY_PROGRESS, downloading: true },
    })
    await markPrompted()
    await call('cmd_start_optional_components_download')
  },
}))
