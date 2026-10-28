import { useState } from 'react'
import { Button, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, InputBase, Stack, Typography } from '../../../ui'
import { SUBAGENT_COLORS } from '../../settings/lib/subagentProfiles'
import { useTeamStore, blankAgent, type TeamAgent } from '../store/teamStore'
import type { FaceStyle } from '../store/faceStore'
import { ModelPicker } from '../../ai/components/ModelPicker'
import { FaceEditor } from './FaceEditor'
import { Avatar, colorOf } from './bits'

const inputSx = { fontSize: '13px', px: '12px', py: '8px', border: 1, borderColor: 'divider', borderRadius: '8px', '&.ui-focused': { borderColor: 'primary.main' } } as const

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <Stack sx={{ gap: '4px' }}>
      <Typography sx={{ fontSize: '13px', fontWeight: 600 }}>{label}</Typography>
      {hint && <Typography sx={{ fontSize: '11px', color: 'text.secondary' }}>{hint}</Typography>}
      {children}
    </Stack>
  )
}

/** Make a 24/7 agent: its name and job, how it looks, its model, and the
 * standing instructions it works from. Schedules, inbox and phone are set on
 * its page once it exists. */
export function NewAgentDialog({ root, open, count, onClose, onCreated }: { root: string; open: boolean; count: number; onClose: () => void; onCreated?: (agent: TeamAgent) => void }) {
  const save = useTeamStore((s) => s.save)
  const [name, setName] = useState('')
  const [role, setRole] = useState('')
  const [color, setColor] = useState<string>(SUBAGENT_COLORS[count % SUBAGENT_COLORS.length])
  const [face, setFace] = useState<FaceStyle | null>(null)
  const [model, setModel] = useState<TeamAgent['model']>(null)
  const [instructions, setInstructions] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const create = async () => {
    if (!name.trim()) return
    setSaving(true)
    setError(null)
    try {
      const made = await save(root, { ...blankAgent(), name: name.trim(), role: role.trim(), color, face, model, instructions, status: 'active', proposedBy: 'user' }, null)
      setName('')
      setRole('')
      setInstructions('')
      onCreated?.(made)
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ display: 'flex', alignItems: 'center', gap: '12px', fontSize: '16px', fontWeight: 600 }}>
        <Avatar name={name || '?'} color={colorOf({ color })} face={face} size={32} />
        New 24/7 agent
      </DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: '16px', pt: '4px' }}>
          <Field label="Name">
            <InputBase autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Inbox Assistant" sx={inputSx} />
          </Field>
          <Field label="Job" hint="One sentence: what it is there to do.">
            <InputBase value={role} onChange={(e) => setRole(e.target.value)} placeholder="Reads my email each morning and drafts replies" sx={inputSx} />
          </Field>
          <Field label="Look">
            <FaceEditor
              name={name}
              color={color}
              face={face}
              onChange={(next) => {
                setColor(next.color)
                setFace(next.face)
              }}
            />
          </Field>
          <Field label="Model">
            <ModelPicker mode="agent" variant="field" value={model} onChange={setModel} />
          </Field>
          <Field label="Instructions" hint="How it should work, every time.">
            <InputBase multiline minRows={4} value={instructions} onChange={(e) => setInstructions(e.target.value)} placeholder={'Check the inbox at 8am.\nNever send anything without asking me first.'} sx={{ ...inputSx, alignItems: 'flex-start', lineHeight: 1.5 }} />
          </Field>
          {error && <Typography sx={{ fontSize: '12px', color: 'error.main' }}>{error}</Typography>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button color="inherit" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="contained" disableElevation disabled={saving || !name.trim()} onClick={() => void create()}>
          {saving ? <CircularProgress size={14} /> : 'Create agent'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
