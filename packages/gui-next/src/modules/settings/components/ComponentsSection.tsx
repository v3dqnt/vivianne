import { useEffect } from 'react'
import { Stack, Typography, Button, LinearProgress, Divider } from '../../../ui'
import { TickCircleLinear, ImportLinear } from '../../../icons'
import { useOptionalComponentsStore } from '../store/optionalComponentsStore'

function Row({
  title,
  size,
  installed,
  installedLabel,
  progress,
  onDownload,
}: {
  title: string
  size: string
  installed: boolean
  installedLabel: string
  progress: { percent: number | null; message: string; error: string | null; downloading: boolean }
  onDownload: () => void
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
        <Typography variant="caption" color="error.main">
          {progress.error}
        </Typography>
      )}
    </Stack>
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
        Two larger, genuinely optional downloads — neither is required for coding tasks.
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
    </Stack>
  )
}
