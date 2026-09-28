import { useEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, IconButton, Tooltip, CircularProgress, Button, alpha, useTheme } from '../../../ui'
import { CloseLinear, DangerLinear } from '../../../icons'
import { ResizeHandle } from '../../../components/ResizeHandle'
import { IdeTree } from './IdeTree'
import { CodeEditor } from './CodeEditor'
import { useIdeStore, isDirty, type IdeTab } from '../store/ideStore'
import { subscribeFileChanged } from '../lib/fileChangeBus'
import { loadPersisted, savePersisted } from '../../../lib/persist'

function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() || p
}

const TREE_WIDTH_KEY = 'ide.treeWidth'
const TREE_MIN = 160
const TREE_MAX = 480

/** One open file's tab strip entry. A warning glyph for "changed on disk"
 * (only one of it or the dirty dot can be true-and-shown meaningfully at
 * once, but both render independently since they answer different
 * questions: "did the agent touch this" vs "do I have unsaved edits"), and
 * hovering swaps the dirty dot for a close × — the convention every editor
 * this app's users already know from elsewhere. */
function Tab({ tab, active, onSelect, onClose }: { tab: IdeTab; active: boolean; onSelect: () => void; onClose: () => void }) {
  const theme = useTheme()
  const dirty = isDirty(tab)
  return (
    <Stack
      direction="row"
      onClick={onSelect}
      sx={{
        alignItems: 'center',
        gap: 0.5,
        pl: 1.25,
        pr: 0.75,
        py: 0.5,
        cursor: 'pointer',
        borderRight: 1,
        borderColor: 'divider',
        bgcolor: active ? 'background.paper' : 'transparent',
        borderBottom: 2,
        borderBottomColor: active ? 'primary.main' : 'transparent',
        flexShrink: 0,
        '&:hover .ide-tab-close': { visibility: 'visible' },
        '&:hover .ide-tab-dot': { visibility: 'hidden' },
      }}
    >
      {tab.changedOnDisk && (
        <Tooltip title="Changed on disk since you opened it">
          <Box sx={{ display: 'flex' }}>
            <DangerLinear size={11} color={theme.palette.warning.main} />
          </Box>
        </Tooltip>
      )}
      <Typography variant="caption" noWrap sx={{ maxWidth: 160, color: active ? 'text.primary' : 'text.secondary' }}>
        {basename(tab.path)}
      </Typography>
      <Box sx={{ width: 14, height: 14, flexShrink: 0, position: 'relative' }}>
        {dirty && (
          <Box
            className="ide-tab-dot"
            sx={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
          >
            <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: 'text.secondary' }} />
          </Box>
        )}
        <IconButton
          className="ide-tab-close"
          size="small"
          onClick={(e) => {
            e.stopPropagation()
            onClose()
          }}
          sx={{
            position: 'absolute',
            inset: 0,
            p: 0,
            visibility: dirty ? 'hidden' : 'visible',
            color: 'text.disabled',
            '&:hover': { color: 'text.primary' },
          }}
        >
          <CloseLinear size={11} color="currentColor" />
        </IconButton>
      </Box>
    </Stack>
  )
}

/** One tab's body: loading / error / the live editor, plus the
 * changed-on-disk banner when the agent wrote this file while it was open. */
function TabBody({ tab }: { tab: IdeTab }) {
  const theme = useTheme()
  const setDraft = useIdeStore((s) => s.setDraft)
  const save = useIdeStore((s) => s.save)
  const reload = useIdeStore((s) => s.reload)
  const markChangedOnDisk = useIdeStore((s) => s.markChangedOnDisk)

  // Subscribed per open tab, not globally: only a file that's actually open
  // here needs to know it was written elsewhere.
  useEffect(() => {
    return subscribeFileChanged(tab.path, () => markChangedOnDisk(tab.path))
  }, [tab.path, markChangedOnDisk])

  if (tab.loading) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
        <CircularProgress size={18} />
      </Stack>
    )
  }
  if (tab.error) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 1, p: 3 }}>
        <Typography variant="body2" color="error.main" sx={{ textAlign: 'center' }}>
          {tab.error}
        </Typography>
      </Stack>
    )
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      {tab.changedOnDisk && (
        <Stack
          direction="row"
          sx={{
            alignItems: 'center',
            gap: 1,
            px: 1.5,
            py: 0.625,
            flexShrink: 0,
            bgcolor: alpha(theme.palette.warning.main, theme.palette.mode === 'dark' ? 0.15 : 0.1),
            borderBottom: 1,
            borderColor: 'divider',
          }}
        >
          <DangerLinear size={13} color={theme.palette.warning.main} />
          <Typography variant="caption" sx={{ flex: 1 }}>
            {isDirty(tab)
              ? 'The agent wrote to this file while you had unsaved changes here. Saving now would overwrite what it did.'
              : 'The agent wrote to this file while it was open.'}
          </Typography>
          <Button size="small" onClick={() => void reload(tab.path)} sx={{ textTransform: 'none', minWidth: 0, px: 1, py: 0.25, fontSize: '0.75rem' }}>
            Reload
          </Button>
        </Stack>
      )}
      <Box sx={{ flex: 1, minHeight: 0 }}>
        <CodeEditor
          key={tab.path}
          path={tab.path}
          initialContent={tab.draft}
          onChange={(content) => setDraft(tab.path, content)}
          onSave={() => void save(tab.path)}
        />
      </Box>
    </Stack>
  )
}

/**
 * A real code editor inside the app — a file tree and an editable,
 * syntax-highlighted CodeMirror buffer for each open file, with Ctrl+S to
 * save and a warning when the agent edits a file you already have open.
 *
 * Distinct from the existing Files dock panel (`WorkspaceTree`), which is a
 * read-only browser: clicking a file there expands a preview inline, on
 * purpose, for quick browsing without leaving the tree. This is for actually
 * writing code — a person editing alongside the agent, not just watching it.
 */
export function IdeView({ showTree = true }: { showTree?: boolean } = {}) {
  const tabs = useIdeStore((s) => s.tabs)
  const activePath = useIdeStore((s) => s.activePath)
  const setActive = useIdeStore((s) => s.setActive)
  const closeTab = useIdeStore((s) => s.closeTab)

  const treeRef = useRef<HTMLDivElement | null>(null)
  const [treeWidth, setTreeWidth] = useState(() => loadPersisted(TREE_WIDTH_KEY, 220))

  const active = tabs.find((t) => t.path === activePath) ?? null

  return (
    <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
      {/* Hidden in the IDE *layout*, where the workspace tree is the left
          dock panel and this is only the editor half. Keeping one component
          for both means an open buffer survives switching layouts. */}
      {showTree && (
        <Box ref={treeRef} sx={{ width: treeWidth, flexShrink: 0, display: 'flex', minHeight: 0, position: 'relative' }}>
          <IdeTree />
          <ResizeHandle
            edge="left"
            size={treeWidth}
            min={TREE_MIN}
            max={TREE_MAX}
            targetRef={treeRef}
            onResize={(next) => {
              setTreeWidth(next)
              savePersisted(TREE_WIDTH_KEY, next)
            }}
          />
        </Box>
      )}
      <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0, ...(showTree ? { borderLeft: 1, borderColor: 'divider' } : {}) }}>
        {tabs.length > 0 && (
          <Stack direction="row" sx={{ flexShrink: 0, overflowX: 'auto', borderBottom: 1, borderColor: 'divider', bgcolor: 'action.hover' }}>
            {tabs.map((t) => (
              <Tab key={t.path} tab={t} active={t.path === activePath} onSelect={() => setActive(t.path)} onClose={() => closeTab(t.path)} />
            ))}
          </Stack>
        )}
        {active ? (
          <TabBody tab={active} />
        ) : (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 0.5 }}>
            <Typography variant="body2" color="text.secondary">
              Select a file to start editing.
            </Typography>
            <Typography variant="caption" color="text.disabled">
              Ctrl/Cmd+S saves the active file.
            </Typography>
          </Stack>
        )}
      </Stack>
    </Stack>
  )
}
