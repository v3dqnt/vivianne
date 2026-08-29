import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

export type TtsEngine = 'cartesia' | 'kokoro' | 'browser'

interface VoiceSettings {
  enabled: boolean
  ttsEngine: TtsEngine
  cartesiaApiKey: string
  cartesiaVoiceId: string
  kokoroVoice: string
  whisperModel: string
}

const DEFAULT_SETTINGS: VoiceSettings = {
  enabled: false,
  ttsEngine: 'cartesia',
  cartesiaApiKey: '',
  cartesiaVoiceId: '',
  kokoroVoice: 'af_heart',
  whisperModel: 'tiny.en',
}

const STORAGE_KEY = 'viv.voice'

/** Session lifecycle. No wake-word states — a session is opened explicitly
 * (hotkey or notch click) rather than detected from passive listening, so
 * there's nothing between "idle" and "listening for the first utterance". */
export type VoiceStatus = 'idle' | 'starting' | 'listening' | 'transcribing' | 'responding' | 'speaking' | 'error'

interface VoiceStoreState extends VoiceSettings {
  status: VoiceStatus
  transcript: string
  lastReply: string
  errorMessage: string | null
  setSettings: (patch: Partial<VoiceSettings>) => void
  setStatus: (status: VoiceStatus) => void
  setTranscript: (text: string) => void
  setLastReply: (text: string) => void
  setError: (message: string | null) => void
}

/** Settings persist via localStorage (shared origin across the main window
 * and the `notch` webview — Tauri windows loading the same dist share
 * storage), same pattern as `themeStore`. Live session state (`status`,
 * `transcript`, ...) is NOT persisted and does not need cross-window sync:
 * only the `notch` window ever runs the pipeline, so it's the only reader.
 *
 * Sharing *storage* isn't the same as sharing *state*: each window runs its
 * own JS realm with its own `create()` call, so the notch window's copy of
 * these settings is a snapshot frozen at whenever its bundle first loaded
 * (app startup, since the notch webview exists — hidden — for the app's
 * whole lifetime). Flipping "Voice mode" on in Settings, in the main
 * window, writes localStorage and updates the MAIN window's store — the
 * already-running notch window never sees it, so `enabled` there stays
 * stuck at `false` and Ctrl+Shift+V silently no-ops forever. The `storage`
 * listener below is what makes a change in one window's Settings actually
 * reach the other window's copy: it only fires in *other* same-origin
 * windows, never the one that made the write, which is exactly the missing
 * half. (The old, pre-port notch page sidestepped this differently — it
 * polled the backend's settings command every 3s instead of using
 * localStorage at all. `storage` events are the equivalent fix without
 * reintroducing a poll loop.) */
export const useVoiceStore = create<VoiceStoreState>((set, get) => ({
  ...loadPersisted<VoiceSettings>(STORAGE_KEY, DEFAULT_SETTINGS),
  status: 'idle',
  transcript: '',
  lastReply: '',
  errorMessage: null,

  setSettings: (patch) => {
    const next = { ...pickSettings(get()), ...patch }
    savePersisted(STORAGE_KEY, next)
    set(next)
  },
  setStatus: (status) => set({ status }),
  setTranscript: (transcript) => set({ transcript }),
  setLastReply: (lastReply) => set({ lastReply }),
  setError: (errorMessage) => set({ errorMessage }),
}))

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (e) => {
    if (e.key === STORAGE_KEY) {
      useVoiceStore.setState(loadPersisted<VoiceSettings>(STORAGE_KEY, DEFAULT_SETTINGS))
    }
  })
}

function pickSettings(s: VoiceStoreState): VoiceSettings {
  const { enabled, ttsEngine, cartesiaApiKey, cartesiaVoiceId, kokoroVoice, whisperModel } = s
  return { enabled, ttsEngine, cartesiaApiKey, cartesiaVoiceId, kokoroVoice, whisperModel }
}
