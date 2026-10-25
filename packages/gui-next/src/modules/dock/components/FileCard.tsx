import { useState } from 'react'
import { Box, IconButton, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { DocumentTextLinear, CopyLinear, TickCircleLinear, MonitorLinear, ArrowRightLinear } from '../../../icons'
import { CHAT_HEADER_SX, CHAT_RADIUS_INNER, CHAT_SURFACE_SX, CHAT_TITLE_SX } from '../../ai/lib/chatStyle'
import { usePanelStore } from '../store/panelStore'
import { FileContent } from './FilePreview'
import { baseName } from '../../ai/components/FileLink'

/** The folder part of a path, for the muted line beside the name. */
function dirOf(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return i > 0 ? path.slice(0, i) : ''
}

/**
 * A file, the way the read tool shows one in chat: a card with its name and
 * where it lives, actions on the right, and its contents in a tinted inset
 * (small, quiet code that folds when long). One look wherever a file is
 * previewed.
 */
export function FileCard({ path, onExpand }: { path: string; onExpand?: () => void }) {
  const theme = useTheme()
  const [copied, setCopied] = useState(false)
  const name = baseName(path)
  const dir = dirOf(path)
  return (
    <Box sx={{ ...CHAT_SURFACE_SX, borderRadius: CHAT_RADIUS_INNER, minWidth: 0 }}>
      <Box sx={{ ...CHAT_HEADER_SX, minHeight: 34, py: 0.5, pr: 0.75 }}>
        <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
          <DocumentTextLinear size={15} color="currentColor" />
        </Box>
        <Typography noWrap sx={{ ...CHAT_TITLE_SX, flexShrink: 1, minWidth: 0 }} title={path}>
          {name}
        </Typography>
        {dir && (
          <Typography noWrap sx={{ fontSize: '0.6875rem', color: 'text.disabled', fontFamily: '"Geist Mono", ui-monospace, monospace', minWidth: 0, flex: 1, direction: 'rtl', textAlign: 'left' }} title={dir}>
            {dir}
          </Typography>
        )}
        {!dir && <Box sx={{ flex: 1 }} />}
        <Tooltip title={copied ? 'Copied' : 'Copy path'}>
          <IconButton
            size="small"
            aria-label="Copy path"
            onClick={() =>
              void navigator.clipboard.writeText(path).then(() => {
                setCopied(true)
                window.setTimeout(() => setCopied(false), 1200)
              })
            }
            sx={{ width: 24, height: 24 }}
          >
            {copied ? <TickCircleLinear size={13} color="currentColor" /> : <CopyLinear size={13} color="currentColor" />}
          </IconButton>
        </Tooltip>
        {onExpand && (
          <Tooltip title="Full screen">
            <IconButton size="small" aria-label="Full screen" onClick={onExpand} sx={{ width: 24, height: 24 }}>
              <MonitorLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
        <Tooltip title="Open in Preview">
          <IconButton size="small" aria-label="Open in Preview" onClick={() => usePanelStore.getState().openDocument(path)} sx={{ width: 24, height: 24 }}>
            <ArrowRightLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Box>
      <Box
        sx={{
          borderTop: 1,
          borderColor: 'divider',
          bgcolor: alpha(theme.palette.text.primary, 0.02),
          px: 1.5,
          py: 1,
          color: 'text.secondary',
          maxHeight: 360,
          overflow: 'auto',
          minWidth: 0,
        }}
      >
        <FileContent path={path} bare dense />
      </Box>
    </Box>
  )
}
