import { useCallback, useEffect, useState } from 'react'
import { Stack, Typography, Button, ButtonBase, LinearProgress, Divider, Alert } from '../../../ui'
import { TickCircleLinear, ImportLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { useOptionalComponentsStore } from '../store/optionalComponentsStore'

function Row({
  title,
  size,
  installed,
  installedLabel,
  progress,
  onDownload,
  onRemove,
}: {
  title: string
  size: string
  installed: boolean
  installedLabel: string
  progress: { percent: number | null; message: string; error: string | null; downloading: boolean }
  onDownload: () => void
  onRemove?: () => void
}) {
  return (
    <Stack sx={{ gap: 1, py: 2 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Stack>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {title}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {size}
          </Typography>
        </Stack>
        {installed ? (
          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
            <TickCircleLinear size={15} color="currentColor" />
            <Typography variant="caption">{installedLabel}</Typography>
            {onRemove && (
              <ButtonBase
                onClick={onRemove}
                sx={{ ml: 1, px: 0.75, py: 0.25, borderRadius: '6px', fontSize: '0.6875rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'error.main' } }}
              >
                Remove
              </ButtonBase>
            )}
          </Stack>
        ) : progress.downloading ? null : (
          <Button size="small" variant="outlined" startIcon={<ImportLinear size={14} color="currentColor" />} onClick={onDownload}>
            Download
          </Button>
        )}
      </Stack>
      {progress.downloading && (
        <Stack sx={{ gap: 0.5 }}>
          <LinearProgress variant={progress.percent === null ? 'indeterminate' : 'determinate'} value={progress.percent ?? 0} />
          <Typography variant="caption" color="text.secondary">
            {progress.message || 'Working...'}
          </Typography>
        </Stack>
      )}
      {progress.error && (
        <Alert severity="error">
          {progress.error}
        </Alert>
      )}
    </Stack>
  )
}

type Progress = { percent: number | null; message: string; error: string | null; downloading: boolean }
const IDLE: Progress = { percent: null, message: '', error: null, downloading: false }

function mb(bytes: number): string {
  return `${Math.round(bytes / 1_000_000)} MB`
}

/** Parakeet, Viv Instant's speech recognizer, as a component: downloaded
 * here ahead of time instead of on the first voice command. The same
 * download Viv Instant starts, so progress shows here whichever began it. */
function ParakeetRow() {
  const [status, setStatus] = useState<{ installed: boolean; bytes: number } | null>(null)
  const [progress, setProgress] = useState<Progress>(IDLE)

  const refresh = useCallback(async () => {
    setStatus(await call<{ installed: boolean; bytes: number }>('cmd_parakeet_status').catch(() => null))
  }, [])

  useEffect(() => {
    void refresh()
    const offs = [
      on<{ received: number; total: number; index: number; count: number; file: string }>('instant:stt_download', (p) => {
        setProgress({
          downloading: true,
          error: null,
          percent: p.total > 0 ? Math.round((p.received / p.total) * 100) : null,
          message: `File ${p.index} of ${p.count} (${p.file}) · ${mb(p.received)}${p.total ? ` of ${mb(p.total)}` : ''}`,
        })
      }),
      on('instant:stt_download_done', () => {
        setProgress(IDLE)
        void refresh()
      }),
    ]
    return () => offs.forEach((p) => void p.then((off) => off()))
  }, [refresh])

  const download = async () => {
    setProgress({ ...IDLE, downloading: true, message: 'Starting the download…' })
    try {
      await call('cmd_parakeet_download')
      setProgress(IDLE)
    } catch (e) {
      setProgress({ ...IDLE, error: e instanceof Error ? e.message : String(e) })
    }
    void refresh()
  }

  const remove = async () => {
    try {
      await call('cmd_parakeet_remove')
    } catch (e) {
      setProgress({ ...IDLE, error: e instanceof Error ? e.message : String(e) })
    }
    void refresh()
  }

  return (
    <Row
      title="Speech recognition (Parakeet TDT 0.6B)"
      size="~628 MB — Viv Instant's most accurate voice model, on-device"
      installed={status?.installed ?? false}
      installedLabel={status?.bytes ? `Installed · ${mb(status.bytes)}` : 'Installed'}
      progress={progress}
      onDownload={() => void download()}
      onRemove={() => void remove()}
    />
  )
}

/** Manual install/retry for the two optional components — same data and
 * download flow as the first-launch prompt (`OptionalComponentsModal`), for
 * anyone who chose "Later" there or wants to check status/retry a failure. */
export function ComponentsSection() {
  const init = useOptionalComponentsStore((s) => s.init)
  const camoufoxInstalled = useOptionalComponentsStore((s) => s.camoufoxInstalled)
  const docker = useOptionalComponentsStore((s) => s.docker)
  const camoufox = useOptionalComponentsStore((s) => s.camoufox)
  const dockerProgress = useOptionalComponentsStore((s) => s.dockerProgress)
  const startDownload = useOptionalComponentsStore((s) => s.startDownload)

  useEffect(() => {
    void init()
  }, [init])

  return (
    <Stack sx={{ gap: 0 }}>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
        Larger, optional downloads. None is required for coding tasks.
      </Typography>
      <Divider />
      <Row
        title="Browser engine (Camoufox)"
        size="~300MB — for web browsing, previews, and search"
        installed={camoufoxInstalled}
        installedLabel="Installed"
        progress={camoufox}
        onDownload={() => void startDownload()}
      />
      <Divider />
      <Row
        title="Docker Desktop"
        size="~500MB — for running untrusted code in a sandbox"
        installed={docker?.installed ?? false}
        installedLabel={docker?.running ? 'Installed & running' : 'Installed'}
        progress={dockerProgress}
        onDownload={() => void startDownload()}
      />
      <Divider />
      <ParakeetRow />
      <Divider />
    </Stack>
  )
}
