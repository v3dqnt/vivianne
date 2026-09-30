import { useEffect, useMemo, useState } from 'react'
import { Stack, Typography, ButtonBase, Popover, InputBase, CircularProgress } from '../../../ui'
import { Hierarchy2Linear, SearchNormalLinear, TickCircleLinear, AddLinear, ArrowDownLinear } from '../../../icons'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useGitStore } from '../../git/store/gitStore'
import { GitGraphModal } from '../../git/components/GitGraphModal'

/** Branch pill in the topbar — click to search/switch branches, create a
 * new one off HEAD, or open the full commit graph. Was a plain label before
 * (no checkout command existed); now backed by `cmd_git_checkout_branch`/
 * `cmd_git_create_branch`. */
export function BranchIndicator() {
  const branch = useWorkspaceStore((s) => s.branch)
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const branches = useGitStore((s) => s.branches)
  const status = useGitStore((s) => s.status)
  const refresh = useGitStore((s) => s.refresh)
  const checkoutBranch = useGitStore((s) => s.checkoutBranch)
  const createBranch = useGitStore((s) => s.createBranch)

  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [graphOpen, setGraphOpen] = useState(false)

  useEffect(() => {
    if (anchor && root) void refresh(root)
  }, [anchor, root, refresh])

  const filtered = useMemo(() => {
    const local = branches.filter((b) => !b.isRemote)
    if (!query.trim()) return local
    const q = query.trim().toLowerCase()
    return local.filter((b) => b.name.toLowerCase().includes(q))
  }, [branches, query])

  const close = () => {
    setAnchor(null)
    setQuery('')
    setCreating(false)
    setNewName('')
    setError(null)
  }

  const switchTo = async (name: string) => {
    if (!root) return
    setBusy(name)
    setError(null)
    try {
      await checkoutBranch(root, name)
      close()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(null)
    }
  }

  const submitCreate = async () => {
    if (!root || !newName.trim()) return
    setBusy('__new__')
    setError(null)
    try {
      await createBranch(root, newName.trim())
      close()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(null)
    }
  }

  if (!branch) return null

  return (
    <>
      <ButtonBase
        onClick={(e) => setAnchor(e.currentTarget)}
        sx={{ display: 'flex', alignItems: 'center', gap: 0.5, px: 1, py: 0.375, borderRadius: 999, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
      >
        <Hierarchy2Linear size={13} color="currentColor" />
        <Typography className="cq-label-sm" variant="caption" noWrap sx={{ fontWeight: 500 }} title={branch}>
          {branch}
        </Typography>
        <ArrowDownLinear size={10} color="currentColor" />
      </ButtonBase>

      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={close}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        transformOrigin={{ vertical: 'top', horizontal: 'left' }}
        slotProps={{ paper: { sx: { width: 320, borderRadius: 2, overflow: 'hidden' } } }}
      >
        <Stack sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 1, borderBottom: 1, borderColor: 'divider' }} direction="row">
          <SearchNormalLinear size={13} color="currentColor" />
          <InputBase autoFocus fullWidth placeholder="Search branches" value={query} onChange={(e) => setQuery(e.target.value)} sx={{ fontSize: '0.8125rem' }} />
        </Stack>

        <Stack sx={{ maxHeight: 280, overflowY: 'auto', py: 0.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ px: 1.5, py: 0.5, fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.6875rem' }}>
            Branches
          </Typography>
          {filtered.length === 0 ? (
            <Typography variant="caption" color="text.secondary" sx={{ px: 1.5, py: 1 }}>
              No matching branches.
            </Typography>
          ) : (
            filtered.map((b) => (
              <ButtonBase
                key={b.name}
                onClick={() => !b.isCurrent && void switchTo(b.name)}
                disabled={busy !== null}
                sx={{ display: 'flex', alignItems: 'flex-start', gap: 1, px: 1.5, py: 0.75, textAlign: 'left', '&:hover': { bgcolor: 'action.hover' } }}
              >
                <Hierarchy2Linear size={13} color="currentColor" style={{ marginTop: 2, flexShrink: 0 }} />
                <Stack sx={{ flex: 1, minWidth: 0 }}>
                  <Typography variant="body2" sx={{ fontSize: '0.8125rem', fontWeight: b.isCurrent ? 700 : 400 }} noWrap>
                    {b.name}
                  </Typography>
                  {b.isCurrent && status.length > 0 && (
                    <Typography variant="caption" color="text.secondary">
                      Uncommitted changes: {status.length} file{status.length === 1 ? '' : 's'}
                    </Typography>
                  )}
                </Stack>
                {busy === b.name ? <CircularProgress size={13} /> : b.isCurrent && <TickCircleLinear size={14} color="currentColor" />}
              </ButtonBase>
            ))
          )}
        </Stack>

        {error && (
          <Typography variant="caption" color="error.main" sx={{ display: 'block', px: 1.5, py: 0.75 }}>
            {error}
          </Typography>
        )}

        <Stack sx={{ borderTop: 1, borderColor: 'divider', py: 0.5 }}>
          {creating ? (
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.5, py: 0.5 }}>
              <InputBase
                autoFocus
                fullWidth
                placeholder="new-branch-name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && void submitCreate()}
                sx={{ fontSize: '0.8125rem', bgcolor: 'action.hover', borderRadius: 1, px: 1, py: 0.375 }}
              />
              <ButtonBase onClick={() => void submitCreate()} disabled={!newName.trim() || busy !== null} sx={{ px: 1, py: 0.5, borderRadius: 1, bgcolor: 'primary.main', color: 'primary.contrastText', fontSize: '0.75rem' }}>
                {busy === '__new__' ? <CircularProgress size={12} sx={{ color: 'inherit' }} /> : 'Create'}
              </ButtonBase>
            </Stack>
          ) : (
            <ButtonBase onClick={() => setCreating(true)} sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 1.5, py: 0.75, width: '100%', justifyContent: 'flex-start', '&:hover': { bgcolor: 'action.hover' } }}>
              <AddLinear size={13} color="currentColor" />
              <Typography variant="body2" sx={{ fontSize: '0.8125rem' }}>
                Create and switch to new branch…
              </Typography>
            </ButtonBase>
          )}
          <ButtonBase
            onClick={() => {
              setGraphOpen(true)
              close()
            }}
            sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 1.5, py: 0.75, width: '100%', justifyContent: 'flex-start', '&:hover': { bgcolor: 'action.hover' } }}
          >
            <Hierarchy2Linear size={13} color="currentColor" />
            <Typography variant="body2" sx={{ fontSize: '0.8125rem' }}>
              Git Graph
            </Typography>
          </ButtonBase>
        </Stack>
      </Popover>

      <GitGraphModal open={graphOpen} onClose={() => setGraphOpen(false)} root={root} />
    </>
  )
}
