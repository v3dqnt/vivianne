import { useState, type ReactNode } from 'react'
import {
  Alert, Box, Button, ButtonBase, ButtonGroup, Checkbox, Chip, CircularProgress, Collapse, Dialog, DialogActions, DialogContent,
  DialogContentText, DialogTitle, Divider, FormControl, FormControlLabel, IconButton, InputBase, InputLabel, LinearProgress, Link,
  ListItemIcon, ListSubheader, Menu, MenuItem, Orb, Paper, Popover, Select, Slider, Snackbar, Stack, Switch, TextField,
  ToggleButton, ToggleButtonGroup, Tooltip, Typography, type OrbState,
} from '../ui'
import { AgentMark } from '../ui/brand/AgentMark'
import { SettingsLinear } from '../icons'

function Demo({ name, children }: { name: string; children: ReactNode }) {
  return (
    <Paper variant="outlined" sx={{ p: '16px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
      <Typography sx={{ fontSize: '0.75rem', fontWeight: 500, color: 'text.secondary', letterSpacing: '0.04em' }}>{name}</Typography>
      <Stack direction="row" sx={{ gap: '12px', flexWrap: 'wrap', alignItems: 'center' }}>{children}</Stack>
    </Paper>
  )
}

const ORB_STATES: OrbState[] = ['working', 'searching', 'solving', 'listening', 'connecting', 'weaving', 'composing', 'breathing', 'shaping']

/** Every component of the UI kit, in its states. */
export function KitSection() {
  const [dialog, setDialog] = useState(false)
  const [snack, setSnack] = useState(false)
  const [open, setOpen] = useState(true)
  const [toggle, setToggle] = useState('a')
  const [menuAt, setMenuAt] = useState<HTMLElement | null>(null)
  const [popAt, setPopAt] = useState<HTMLElement | null>(null)
  const [sel, setSel] = useState('one')
  const [level, setLevel] = useState(40)
  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(360px, 1fr))', gap: '16px' }}>
      <Demo name="Typography, Link">
        <Stack sx={{ gap: '4px' }}>
          <Typography variant="h5">Heading five</Typography>
          <Typography variant="subtitle1">Subtitle</Typography>
          <Typography>Body text, the default.</Typography>
          <Typography variant="caption" sx={{ color: 'text.secondary' }}>Caption and secondary colour</Typography>
          <Link href="#">A link</Link>
        </Stack>
      </Demo>
      <Demo name="Button">
        {(['contained', 'outlined', 'text'] as const).map((v) => (
          <Button key={v} variant={v}>{v}</Button>
        ))}
        <Button size="small" variant="outlined">small</Button>
        <Button variant="contained" disabled>disabled</Button>
        <Button variant="contained" color="error">error</Button>
      </Demo>
      <Demo name="ButtonBase, IconButton">
        <ButtonBase sx={{ px: '12px', height: 32, borderRadius: '8px', '&:hover': { bgcolor: 'action.hover' } }}>ButtonBase</ButtonBase>
        <IconButton aria-label="settings"><SettingsLinear size={18} /></IconButton>
        <IconButton size="small" disabled><SettingsLinear size={16} /></IconButton>
      </Demo>
      <Demo name="ButtonGroup, ToggleButtonGroup, ToggleButton">
        <ButtonGroup variant="outlined"><Button>One</Button><Button>Two</Button><Button>Three</Button></ButtonGroup>
        <ToggleButtonGroup exclusive value={toggle} onChange={(_, v) => v && setToggle(v)} size="small">
          <ToggleButton value="a">A</ToggleButton>
          <ToggleButton value="b">B</ToggleButton>
          <ToggleButton value="c">C</ToggleButton>
        </ToggleButtonGroup>
      </Demo>
      <Demo name="Chip">
        <Chip label="Default" />
        <Chip label="Outlined" variant="outlined" />
        <Chip label="Small" size="small" />
        <Chip label="Disabled" disabled />
      </Demo>
      <Demo name="TextField, InputBase">
        <TextField label="Label" size="small" defaultValue="Value" />
        <TextField label="Error" size="small" error helperText="Something is wrong" />
        <TextField label="Multiline" size="small" multiline minRows={2} />
        <InputBase placeholder="InputBase" sx={{ border: 1, borderColor: 'divider', borderRadius: '8px', px: '10px', height: 32 }} />
      </Demo>
      <Demo name="Select, FormControl, InputLabel">
        <FormControl size="small" sx={{ minWidth: 160 }}>
          <InputLabel>Choice</InputLabel>
          <Select label="Choice" value={sel} onChange={(e) => setSel(String(e.target.value))}>
            <MenuItem value="one">One</MenuItem>
            <MenuItem value="two">Two</MenuItem>
          </Select>
        </FormControl>
      </Demo>
      <Demo name="Switch, Checkbox, FormControlLabel">
        <FormControlLabel control={<Switch defaultChecked />} label="Switch" />
        <FormControlLabel control={<Checkbox defaultChecked />} label="Checkbox" />
        <FormControlLabel control={<Switch disabled />} label="Disabled" />
      </Demo>
      <Demo name="Slider">
        <Box sx={{ width: 240 }}><Slider value={level} onChange={(_, v) => setLevel(v)} /></Box>
      </Demo>
      <Demo name="Alert">
        {(['info', 'success', 'warning', 'error'] as const).map((s) => (
          <Alert key={s} severity={s} sx={{ width: '100%' }}>{s} alert</Alert>
        ))}
      </Demo>
      <Demo name="CircularProgress, LinearProgress">
        <CircularProgress size={20} />
        <CircularProgress size={32} variant="determinate" value={60} />
        <Box sx={{ width: 200 }}><LinearProgress /></Box>
        <Box sx={{ width: 200 }}><LinearProgress variant="determinate" value={45} /></Box>
      </Demo>
      <Demo name="Tooltip">
        <Tooltip title="A tooltip"><Button variant="outlined">Hover me</Button></Tooltip>
      </Demo>
      <Demo name="Menu, MenuItem, ListItemIcon, ListSubheader">
        <Button variant="outlined" onClick={(e) => setMenuAt(e.currentTarget)}>Open menu</Button>
        <Menu open={!!menuAt} anchorEl={menuAt} onClose={() => setMenuAt(null)}>
          <ListSubheader>Section</ListSubheader>
          <MenuItem onClick={() => setMenuAt(null)}><ListItemIcon><SettingsLinear size={16} /></ListItemIcon>Settings</MenuItem>
          <MenuItem onClick={() => setMenuAt(null)}>Another</MenuItem>
        </Menu>
      </Demo>
      <Demo name="Popover">
        <Button variant="outlined" onClick={(e) => setPopAt(e.currentTarget)}>Open popover</Button>
        <Popover open={!!popAt} anchorEl={popAt} onClose={() => setPopAt(null)}>
          <Box sx={{ p: '12px' }}>Popover content</Box>
        </Popover>
      </Demo>
      <Demo name="Dialog, DialogTitle, DialogContent, DialogContentText, DialogActions">
        <Button variant="outlined" onClick={() => setDialog(true)}>Open dialog</Button>
        <Dialog open={dialog} onClose={() => setDialog(false)}>
          <DialogTitle>Dialog title</DialogTitle>
          <DialogContent><DialogContentText>Dialog body text.</DialogContentText></DialogContent>
          <DialogActions><Button onClick={() => setDialog(false)}>Cancel</Button><Button variant="contained" onClick={() => setDialog(false)}>OK</Button></DialogActions>
        </Dialog>
      </Demo>
      <Demo name="Snackbar">
        <Button variant="outlined" onClick={() => setSnack(true)}>Show snackbar</Button>
        <Snackbar open={snack} autoHideDuration={2500} onClose={() => setSnack(false)} message="Saved" />
      </Demo>
      <Demo name="Collapse, Divider, Stack, Box, Paper">
        <Button variant="outlined" onClick={() => setOpen((o) => !o)}>{open ? 'Collapse' : 'Expand'}</Button>
        <Collapse in={open}><Box sx={{ p: '8px' }}>Collapsible content</Box></Collapse>
        <Divider sx={{ width: '100%' }} />
      </Demo>
      <Demo name="Orb (every state)">
        {ORB_STATES.map((s) => (
          <Stack key={s} sx={{ alignItems: 'center', gap: '4px' }}>
            <Orb state={s} />
            <Typography variant="caption">{s}</Typography>
          </Stack>
        ))}
      </Demo>
      <Demo name="AgentMark">
        {['#e5484d', '#3e63dd', '#30a46c', '#f76b15'].map((c) => <AgentMark key={c} color={c} size={32} title={c} />)}
      </Demo>
    </Box>
  )
}
