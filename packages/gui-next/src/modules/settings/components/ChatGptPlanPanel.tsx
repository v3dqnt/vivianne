import { useEffect, useState } from 'react'
import { Box, Button, ButtonBase, CircularProgress, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { useModelStore } from '../../ai/store/modelStore'

/** Mirrors `ai::chatgpt::ChatGptStatus`. */
export interface ChatGptStatus {
  signedIn: boolean
  email: string | null
  sharing: boolean
  manageUrl: string
}

export async function chatGptStatus(): Promise<ChatGptStatus | null> {
  return call<ChatGptStatus>('cmd_chatgpt_status').catch(() => null)
}

/** OpenAI's mark, for the sign-in button. */
function OpenAiMark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M22.28 9.82a5.98 5.98 0 0 0-.52-4.91 6.05 6.05 0 0 0-6.51-2.9A6.07 6.07 0 0 0 4.98 4.18a5.98 5.98 0 0 0-4 2.9 6.05 6.05 0 0 0 .74 7.1 5.98 5.98 0 0 0 .51 4.91 6.05 6.05 0 0 0 6.51 2.9A5.98 5.98 0 0 0 13.26 24a6.06 6.06 0 0 0 5.77-4.21 5.99 5.99 0 0 0 4-2.9 6.06 6.06 0 0 0-.75-7.07zm-9.02 12.61a4.48 4.48 0 0 1-2.88-1.04l.14-.08 4.78-2.76a.8.8 0 0 0 .39-.68v-6.74l2.02 1.17a.07.07 0 0 1 .04.05v5.58a4.5 4.5 0 0 1-4.49 4.5zM3.6 18.3a4.47 4.47 0 0 1-.53-3.01l.14.08 4.78 2.76a.77.77 0 0 0 .78 0l5.84-3.37v2.33a.08.08 0 0 1-.03.06L9.74 19.95a4.5 4.5 0 0 1-6.14-1.65zM2.34 7.9a4.49 4.49 0 0 1 2.37-1.97V11.6a.77.77 0 0 0 .39.68l5.81 3.35-2.02 1.17a.08.08 0 0 1-.07 0l-4.83-2.79A4.5 4.5 0 0 1 2.34 7.87zm16.6 3.86-5.84-3.39 2.02-1.16a.08.08 0 0 1 .07 0l4.83 2.79a4.49 4.49 0 0 1-.68 8.1v-5.68a.79.79 0 0 0-.4-.66zm2.01-3.02-.14-.09-4.77-2.78a.78.78 0 0 0-.79 0L9.41 9.23V6.9a.07.07 0 0 1 .03-.06l4.83-2.79a4.5 4.5 0 0 1 6.68 4.66zM8.31 12.86l-2.02-1.16a.08.08 0 0 1-.04-.06V6.07a4.5 4.5 0 0 1 7.38-3.45l-.14.08-4.78 2.76a.8.8 0 0 0-.39.68zm1.1-2.37 2.6-1.5 2.6 1.5v3l-2.6 1.5-2.6-1.5z" />
    </svg>
  )
}

/**
 * Use a ChatGPT plan instead of API keys (OpenAI's Sign in with ChatGPT).
 * Wording follows OpenAI's guidelines: "Continue with ChatGPT" to connect,
 * "Using ChatGPT plan" once connected, and "Manage usage" for the weekly cap.
 */
export function ChatGptPlanPanel({ onChange }: { onChange?: (status: ChatGptStatus | null) => void }) {
  const [status, setStatus] = useState<ChatGptStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const apply = (s: ChatGptStatus | null) => {
    setStatus(s)
    onChange?.(s)
  }

  useEffect(() => {
    void chatGptStatus().then(apply)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const run = async (command: string) => {
    setBusy(true)
    setError(null)
    try {
      apply(await call<ChatGptStatus>(command))
      // The plan's models join (or leave) the model picker.
      void useModelStore.getState().init()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const connected = !!status?.signedIn && status.sharing

  return (
    <Stack sx={{ gap: 2, maxWidth: 480 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
          ChatGPT plan
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
          Use your ChatGPT Plus or Pro plan for OpenAI models in Vivianne, instead of an API key. Usage counts against your
          plan, up to a weekly limit you set for Vivianne in ChatGPT.
        </Typography>
      </Stack>

      {connected ? (
        <Stack sx={{ gap: 1.25, p: 1.5, border: 1, borderColor: 'divider', borderRadius: '12px' }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: 'success.main' }} />
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              Using ChatGPT plan
            </Typography>
            {status?.email && (
              <Typography variant="body2" color="text.secondary" noWrap sx={{ minWidth: 0 }}>
                · {status.email}
              </Typography>
            )}
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
            Pick a model marked “ChatGPT plan” in the model picker to use it. When the weekly limit is reached, Vivianne stops
            and tells you rather than switching to paid usage.
          </Typography>
          <Stack direction="row" sx={{ gap: 1 }}>
            <Button size="small" variant="outlined" onClick={() => void call('cmd_chatgpt_manage_usage')} sx={{ borderRadius: '8px' }}>
              Manage usage
            </Button>
            <Button size="small" color="inherit" disabled={busy} onClick={() => void run('cmd_chatgpt_sign_out')} sx={{ borderRadius: '8px', color: 'text.secondary' }}>
              Sign out
            </Button>
          </Stack>
        </Stack>
      ) : (
        <Stack sx={{ gap: 1 }}>
          {status?.signedIn && !status.sharing && (
            <Typography variant="caption" color="warning.main" sx={{ lineHeight: 1.5 }}>
              You're signed in, but using your plan wasn't allowed during sign-in. Continue with ChatGPT again and allow plan
              usage.
            </Typography>
          )}
          <ButtonBase
            disabled={busy}
            onClick={() => void run('cmd_chatgpt_sign_in')}
            sx={{
              alignSelf: 'flex-start',
              gap: 1,
              height: 40,
              px: 2,
              borderRadius: '999px',
              bgcolor: 'text.primary',
              color: 'background.paper',
              fontSize: '0.875rem',
              fontWeight: 600,
              opacity: busy ? 0.7 : 1,
              '&:hover': { opacity: 0.88 },
            }}
          >
            {busy ? <CircularProgress size={14} sx={{ color: 'inherit' }} /> : <OpenAiMark />}
            Continue with ChatGPT
          </ButtonBase>
          {busy && (
            <Typography variant="caption" color="text.secondary">
              Finish signing in in your browser…
            </Typography>
          )}
        </Stack>
      )}

      {error && (
        <Typography variant="caption" color="error.main" sx={{ lineHeight: 1.5 }}>
          {error}
        </Typography>
      )}
    </Stack>
  )
}
