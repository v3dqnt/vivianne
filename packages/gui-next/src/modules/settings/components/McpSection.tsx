import { useEffect, useState } from 'react'
import { Stack, Typography, Divider, Button, ButtonBase, IconButton, CircularProgress, Dialog, TextField, Select, MenuItem, Chip, Collapse, Alert } from '../../../ui'
import { ImportLinear, TrashLinear, Link2Linear, CommandLinear, GlobalLinear, ArrowDownLinear } from '../../../icons'
import { useMcpStore } from '../store/mcpStore'
import { DEFAULT_MCP_TIMEOUT_MS, type McpServerConfig, type McpTransportConfig } from '../lib/mcp'

function ServerRow({ server, toolCount }: { server: McpServerConfig; toolCount: number }) {
  const removeServer = useMcpStore((s) => s.removeServer)
  const [busy, setBusy] = useState(false)
  const isHttp = server.transport.kind === 'http'

  const doRemove = async () => {
    setBusy(true)
    try {
      await removeServer(server.name)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1.25, px: 1.5, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
      {isHttp ? <GlobalLinear size={16} color="currentColor" /> : <CommandLinear size={16} color="currentColor" />}
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
          {server.name}
        </Typography>
        <Typography variant="caption" color="text.secondary" noWrap>
          {server.transport.kind === 'http'
            ? server.transport.url
            : server.transport.kind === 'stdio'
              ? `${server.transport.command} ${server.transport.args.join(' ')}`
              : 'Built in'}
        </Typography>
      </Stack>
      <Chip size="small" label={`${toolCount} tool${toolCount === 1 ? '' : 's'}`} sx={{ fontSize: '0.6875rem' }} />
      <IconButton size="small" onClick={() => void doRemove()} disabled={busy}>
        {busy ? <CircularProgress size={14} /> : <TrashLinear size={15} color="currentColor" />}
      </IconButton>
    </Stack>
  )
}

/** Accepts the common external MCP-config shape (`{"name": {...}}` or
 * `{"mcpServers": {"name": {...}}}`, the same flattened layout Claude
 * Desktop's own config file uses) and converts it into this app's own
 * richer `McpServerConfig`. Only the first entry is used — this dialog adds
 * one server at a time, matching the reference flow. */
function parseExternalConfig(raw: string): McpServerConfig {
  const parsed = JSON.parse(raw) as Record<string, unknown>
  const container = (typeof parsed.mcpServers === 'object' && parsed.mcpServers !== null ? parsed.mcpServers : parsed) as Record<
    string,
    Record<string, unknown>
  >
  const entries = Object.entries(container)
  if (entries.length === 0) throw new Error('No server entries found.')
  const [name, def] = entries[0]
  if (!name.trim()) throw new Error('Server name cannot be empty.')
  const kind = def.type === 'http' ? 'http' : 'stdio'
  const transport: McpTransportConfig =
    kind === 'http'
      ? { kind: 'http', url: typeof def.url === 'string' ? def.url : '', headers: (def.headers as Record<string, string>) ?? {} }
      : {
          kind: 'stdio',
          command: typeof def.command === 'string' ? def.command : '',
          args: Array.isArray(def.args) ? (def.args as string[]) : [],
          env: (def.env as Record<string, string>) ?? {},
        }
  const timeoutMs = typeof def.timeout_ms === 'number' ? def.timeout_ms : typeof def.timeoutMs === 'number' ? def.timeoutMs : null
  return { name, transport, timeoutMs }
}

const JSON_PLACEHOLDER = `{
  "my-mcp-server": {
    "type": "stdio",
    "command": "",
    "args": []
  }
}`

function AddServerDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const addServer = useMcpStore((s) => s.addServer)
  const [mode, setMode] = useState<'form' | 'json'>('form')

  // Form mode fields
  const [name, setName] = useState('')
  const [kind, setKind] = useState<'stdio' | 'http'>('stdio')
  const [timeoutMs, setTimeoutMs] = useState(DEFAULT_MCP_TIMEOUT_MS)
  const [command, setCommand] = useState('')
  const [args, setArgs] = useState('')
  const [url, setUrl] = useState('')
  const [envOpen, setEnvOpen] = useState(false)
  const [envJson, setEnvJson] = useState('')

  // JSON mode
  const [rawJson, setRawJson] = useState('')

  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const reset = () => {
    setName('')
    setKind('stdio')
    setTimeoutMs(DEFAULT_MCP_TIMEOUT_MS)
    setCommand('')
    setArgs('')
    setUrl('')
    setEnvOpen(false)
    setEnvJson('')
    setRawJson('')
    setError(null)
  }

  const buildFromForm = (): McpServerConfig => {
    if (!name.trim()) throw new Error('Name is required.')
    let env: Record<string, string> = {}
    if (envJson.trim()) {
      try {
        env = JSON.parse(envJson) as Record<string, string>
      } catch {
        throw new Error('Environment variables must be valid JSON.')
      }
    }
    const transport: McpTransportConfig =
      kind === 'stdio'
        ? { kind: 'stdio', command: command.trim(), args: args.trim() ? args.trim().split(/\s+/) : [], env }
        : { kind: 'http', url: url.trim(), headers: {} }
    return { name: name.trim(), transport, timeoutMs: timeoutMs || null }
  }

  const submit = async () => {
    setSaving(true)
    setError(null)
    try {
      const config = mode === 'form' ? buildFromForm() : parseExternalConfig(rawJson)
      await addServer(config)
      reset()
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  const canSubmit = mode === 'form' ? !!name.trim() && (kind === 'stdio' ? !!command.trim() : !!url.trim()) : !!rawJson.trim()

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth slotProps={{ paper: { sx: { borderRadius: 3 } } }}>
      <Stack sx={{ p: 3, gap: 2.5 }}>
        <Stack direction="row" sx={{ alignItems: 'flex-start', justifyContent: 'space-between' }}>
          <Stack sx={{ gap: 0.5 }}>
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              New MCP server
            </Typography>
            <Typography variant="caption" color="text.secondary">
              Fill in the new MCP configuration, then save to return to the list.
            </Typography>
          </Stack>
          <Stack direction="row" sx={{ border: 1, borderColor: 'divider', borderRadius: 999, p: 0.25, flexShrink: 0 }}>
            {(['form', 'json'] as const).map((m) => (
              <ButtonBase
                key={m}
                onClick={() => setMode(m)}
                sx={{
                  px: 1.5,
                  py: 0.375,
                  borderRadius: 999,
                  fontSize: '0.75rem',
                  fontWeight: 600,
                  bgcolor: mode === m ? 'action.selected' : 'transparent',
                  color: mode === m ? 'text.primary' : 'text.secondary',
                  transition: 'background-color 0.15s, color 0.15s',
                }}
              >
                {m === 'form' ? 'Form' : 'JSON'}
              </ButtonBase>
            ))}
          </Stack>
        </Stack>

        {mode === 'form' ? (
          <Stack sx={{ gap: 2 }}>
            <TextField label="Name" size="small" placeholder="my-mcp-server" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
            <Stack direction="row" sx={{ gap: 2 }}>
              <Stack sx={{ flex: 1, gap: 0.5 }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  Type
                </Typography>
                <Select size="small" value={kind} onChange={(e) => setKind(e.target.value as 'stdio' | 'http')}>
                  <MenuItem value="stdio">stdio (local command)</MenuItem>
                  <MenuItem value="http">HTTP</MenuItem>
                </Select>
              </Stack>
              <Stack sx={{ width: 140, gap: 0.5 }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  Timeout MS
                </Typography>
                <TextField
                  size="small"
                  type="number"
                  placeholder="30000"
                  value={timeoutMs}
                  onChange={(e) => setTimeoutMs(Math.max(0, Number(e.target.value) || 0))}
                />
              </Stack>
            </Stack>

            {kind === 'stdio' ? (
              <>
                <TextField label="Command" size="small" placeholder="npx" value={command} onChange={(e) => setCommand(e.target.value)} />
                <TextField
                  label="Arguments (space separated)"
                  size="small"
                  placeholder="-y @modelcontextprotocol/server-memory"
                  value={args}
                  onChange={(e) => setArgs(e.target.value)}
                />
              </>
            ) : (
              <TextField label="URL" size="small" placeholder="https://example.com/mcp" value={url} onChange={(e) => setUrl(e.target.value)} />
            )}

            {kind === 'stdio' && (
              <Stack sx={{ gap: 1 }}>
                <ButtonBase
                  onClick={() => setEnvOpen((v) => !v)}
                  sx={{ display: 'flex', alignItems: 'center', gap: 0.5, alignSelf: 'flex-start', color: 'text.secondary', fontSize: '0.8125rem', fontWeight: 600 }}
                >
                  <ArrowDownLinear size={12} color="currentColor" style={{ transform: envOpen ? 'rotate(180deg)' : undefined, transition: 'transform 0.15s' }} />
                  Environment variables (optional)
                </ButtonBase>
                <Collapse in={envOpen}>
                  <TextField
                    fullWidth
                    multiline
                    minRows={3}
                    placeholder={'{\n  "MY_API_KEY": "your-key"\n}'}
                    value={envJson}
                    onChange={(e) => setEnvJson(e.target.value)}
                    sx={{ '& .ui-InputBase-input': { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' } }}
                  />
                </Collapse>
              </Stack>
            )}
          </Stack>
        ) : (
          <Stack sx={{ gap: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              Full configuration
            </Typography>
            <TextField
              fullWidth
              multiline
              minRows={8}
              placeholder={JSON_PLACEHOLDER}
              value={rawJson}
              onChange={(e) => setRawJson(e.target.value)}
              sx={{ '& .ui-InputBase-input': { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' } }}
            />
            <Typography variant="caption" color="text.secondary">
              You can paste {'{"server-name": {...}}'} or {'{"mcpServers": {"server-name": {...}}}'}.
            </Typography>
          </Stack>
        )}

        {error && (
          <Alert severity="error">
            {error}
          </Alert>
        )}

        <Stack direction="row" sx={{ gap: 1 }}>
          <Button
            onClick={() => void submit()}
            variant="contained"
            disableElevation
            disabled={saving || !canSubmit}
            startIcon={saving ? <CircularProgress size={14} /> : undefined}
          >
            Add
          </Button>
          <Button onClick={onClose} color="inherit">
            Cancel
          </Button>
        </Stack>
      </Stack>
    </Dialog>
  )
}

/** `hide`: servers managed in Connections, which aren't listed here. */
export function McpSection({ hide = [] }: { hide?: string[] }) {
  const servers = useMcpStore((s) => s.servers).filter((s) => !hide.includes(s.name))
  const tools = useMcpStore((s) => s.tools)
  const refresh = useMcpStore((s) => s.refresh)
  const [dialogOpen, setDialogOpen] = useState(false)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const toolCountFor = (name: string) => tools.filter(([serverName]) => serverName === name).length

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 0.5 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          MCP servers
        </Typography>
        <Button
          size="small"
          onClick={() => setDialogOpen(true)}
          startIcon={<ImportLinear size={14} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          Add server
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Model Context Protocol servers extend the agent with extra tools — run locally over stdio, or connect to a
        remote server over HTTP (with OAuth if it requires it).
      </Typography>
      <Divider sx={{ mb: 0.5 }} />
      {servers.length === 0 ? (
        <Stack direction="row" spacing={1} sx={{ py: 1, alignItems: 'center', color: 'text.secondary' }}>
          <Link2Linear size={14} color="currentColor" />
          <Typography variant="caption">No MCP servers added yet.</Typography>
        </Stack>
      ) : (
        servers.map((s) => <ServerRow key={s.name} server={s} toolCount={toolCountFor(s.name)} />)
      )}
      <AddServerDialog open={dialogOpen} onClose={() => setDialogOpen(false)} />
    </Stack>
  )
}
