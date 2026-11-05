import { useEffect, useRef, useState } from 'react'
import { Box, Button, ButtonBase, Dialog, DialogActions, DialogContent, DialogTitle, Menu, MenuItem, Stack, Typography } from '../../../ui'
import { ArrowDownLinear } from '../../../icons'
import { useProjectsStore, projectColor, PROJECT_COLORS, type Project } from '../store/projectsStore'
import { bannerBackground, PRESETS, presetBanner } from '../lib/banner'
import { bannerFromFile } from '../lib/bannerImage'
import { ProjectDialog, iconFrom } from './ProjectRail'

export const BANNER_HEIGHT = 112

/**
 * The picture across the top of a project's threads, with its name over it
 * and a menu behind the name, the way a Discord server's header works. A
 * project with no banner of its own gets a gradient from its colour; General
 * is not a project, so its banner is fixed and has no menu.
 */
export function ProjectBanner({ project, name }: { project: Project | null; name: string }) {
  const url = useProjectsStore((s) => (project ? s.bannerUrls[project.id] : null))
  const loadBanner = useProjectsStore((s) => s.loadBanner)
  const remove = useProjectsStore((s) => s.remove)
  const setIcon = useProjectsStore((s) => s.setIcon)
  const openExternal = useProjectsStore((s) => s.openExternal)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [editing, setEditing] = useState(false)
  const [picking, setPicking] = useState(false)
  const iconInput = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (project?.banner?.startsWith('file:')) void loadBanner(project.id)
  }, [project?.id, project?.banner, loadBanner])

  const color = project ? projectColor(project) : PROJECT_COLORS.blue
  const background = bannerBackground(project?.banner, color, url)
  const title = (
    <Typography noWrap sx={{ flex: 1, minWidth: 0, textAlign: 'left', fontSize: '1rem', fontWeight: 600, letterSpacing: '-0.01em', color: '#fff', textShadow: '0 1px 2px rgba(0,0,0,0.5)' }}>
      {name}
    </Typography>
  )

  return (
    <Box sx={{ position: 'relative', height: BANNER_HEIGHT, flexShrink: 0, background, overflow: 'hidden' }}>
      {/* The name sits on a darkening gradient, so it clears 4.5:1 over any picture, light ones included. */}
      <Box aria-hidden sx={{ position: 'absolute', inset: 0, background: 'linear-gradient(180deg, rgba(0,0,0,0) 35%, rgba(0,0,0,0.66) 100%)', pointerEvents: 'none' }} />
      {project ? (
        <ButtonBase
          onClick={(e) => setAnchor(e.currentTarget)}
          aria-haspopup="menu"
          aria-label={`${name}: project menu`}
          sx={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: 44, px: '14px', gap: '8px', justifyContent: 'space-between', color: '#fff', '&:hover': { bgcolor: 'rgba(255,255,255,0.08)' } }}
        >
          {title}
          <ArrowDownLinear size={16} color="currentColor" />
        </ButtonBase>
      ) : (
        <Stack direction="row" sx={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: 44, px: '14px', alignItems: 'center' }}>
          {title}
        </Stack>
      )}

      <Menu anchorEl={anchor} open={!!anchor} onClose={() => setAnchor(null)}>
        <MenuItem onClick={() => { setAnchor(null); setPicking(true) }}>Change banner…</MenuItem>
        <MenuItem onClick={() => { setAnchor(null); setEditing(true) }}>Edit project…</MenuItem>
        <MenuItem onClick={() => { setAnchor(null); iconInput.current?.click() }}>Set icon…</MenuItem>
        {project?.path && <MenuItem onClick={() => { setAnchor(null); void openExternal('explorer') }}>Open folder</MenuItem>}
        <MenuItem onClick={() => { setAnchor(null); if (project) void remove(project.id) }} sx={{ color: 'error.main' }}>Delete project</MenuItem>
      </Menu>

      <input
        ref={iconInput}
        type="file"
        accept="image/*"
        hidden
        onChange={async (e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (file && project) await setIcon(project.id, await iconFrom(file))
        }}
      />
      {project && <ProjectDialog open={editing} project={project} onClose={() => setEditing(false)} />}
      {project && <BannerPicker open={picking} project={project} color={color} onClose={() => setPicking(false)} />}
    </Box>
  )
}

/** Choose a picture of your own, one of the built-in banners, or the gradient. */
function BannerPicker({ open, project, color, onClose }: { open: boolean; project: Project; color: string; onClose: () => void }) {
  const setBanner = useProjectsStore((s) => s.setBanner)
  const file = useRef<HTMLInputElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const apply = async (value: string | null) => {
    setBusy(true)
    setError(null)
    try {
      await setBanner(project.id, value)
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }
  const upload = async (f: File) => {
    try {
      await apply(await bannerFromFile(f))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 600 }}>Banner for {project.name}</DialogTitle>
      <DialogContent>
        <Box
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault()
            const f = e.dataTransfer.files?.[0]
            if (f && f.type.startsWith('image/')) void upload(f)
          }}
          sx={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '8px' }}
        >
          {PRESETS.map((p) => (
            <ButtonBase
              key={p.id}
              disabled={busy}
              onClick={() => void apply(`preset:${p.id}`)}
              aria-label={`${p.name} banner`}
              sx={{
                aspectRatio: '3 / 1',
                borderRadius: '8px',
                background: presetBanner(p.id, color),
                border: 1,
                borderColor: project.banner === `preset:${p.id}` ? 'primary.main' : 'divider',
                outline: project.banner === `preset:${p.id}` ? '2px solid' : 'none',
                outlineColor: 'primary.main',
                outlineOffset: 1,
                alignItems: 'flex-end',
                justifyContent: 'flex-start',
                p: '4px 6px',
                color: '#fff',
                fontSize: '0.6875rem',
                fontWeight: 500,
                textShadow: '0 1px 2px rgba(0,0,0,0.6)',
              }}
            >
              {p.name}
            </ButtonBase>
          ))}
        </Box>
        <Typography sx={{ mt: '12px', fontSize: '0.75rem', color: 'text.secondary' }}>
          Or drop a picture here. It is cropped to 3:1 around its centre and kept small.
        </Typography>
        {error && <Typography sx={{ mt: '8px', fontSize: '0.8125rem', color: 'error.main' }}>{error}</Typography>}
        <input
          ref={file}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/*"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0]
            e.target.value = ''
            if (f) void upload(f)
          }}
        />
      </DialogContent>
      <DialogActions>
        <Button variant="text" disabled={busy || !project.banner} onClick={() => void apply(null)}>
          Use the gradient
        </Button>
        <Box sx={{ flex: 1 }} />
        <Button variant="outlined" disabled={busy} onClick={() => file.current?.click()}>
          Choose a picture…
        </Button>
        <Button variant="text" onClick={onClose}>
          Close
        </Button>
      </DialogActions>
    </Dialog>
  )
}
