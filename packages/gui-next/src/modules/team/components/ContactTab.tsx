import { useEffect, useState } from 'react'
import { Box, Button, Dialog, DialogActions, DialogContent, DialogTitle, InputBase, Link, Stack, Switch, Typography, alpha, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import { SectionLabel } from './bits'
import type { TeamAgent } from '../store/teamStore'

/** Mirrors `contact::Contact`. */
interface Contact {
  email: { inboxId: string; address: string } | null
  phone: { number: string; sid: string } | null
  inbound: boolean
}
interface Providers {
  agentmail: boolean
  twilio: boolean
}
interface NumberOffer {
  numbers: { number: string; label: string; sms: boolean; voice: boolean }[]
  monthlyPrice: string | null
}

const inputSx = { fontSize: '0.8125rem', px: 1.25, py: 0.5, border: 1, borderColor: 'divider', borderRadius: '8px', flex: 1, minWidth: 0 } as const

/**
 * An agent's own email address and phone number (`agent::contact`). With
 * them it gets the `email` and `phone` tools; sending, texting and calling
 * follow its rules for acting outside the machine.
 */
export function ContactTab({ root, agent }: { root: string; agent: TeamAgent }) {
  const theme = useTheme()
  const args = { root, botId: agent.id }
  const [contact, setContact] = useState<Contact | null>(null)
  const [providers, setProviders] = useState<Providers | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [username, setUsername] = useState('')
  const [mailKey, setMailKey] = useState('')
  const [twilioSid, setTwilioSid] = useState('')
  const [twilioToken, setTwilioToken] = useState('')
  const [country, setCountry] = useState('US')
  const [areaCode, setAreaCode] = useState('')
  const [offer, setOffer] = useState<NumberOffer | null>(null)
  const [buying, setBuying] = useState<string | null>(null)
  const [confirm, setConfirm] = useState<'email' | 'phone' | null>(null)

  const refresh = async () => {
    const [c, p] = await Promise.all([call<Contact>('cmd_contact_get', args), call<Providers>('cmd_contact_providers')])
    setContact(c ?? null)
    setProviders(p ?? null)
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, agent.id])

  const run = async (label: string, fn: () => Promise<void>) => {
    setBusy(label)
    setError(null)
    try {
      await fn()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const saveKey = (providerId: string, apiKey: string) => call('auth_store_api_key', { providerId, apiKey: apiKey.trim() })

  if (!contact || !providers) return null

  return (
    <Stack sx={{ gap: 2, pt: 0.5 }}>
      <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
        Give {agent.name} its own email address and phone number, so it can write to people, text and call for you. Sending, texting
        and calling follow its rule for acting outside this machine (Rules tab); reading its own inbox never needs approval.
      </Typography>

      {/* ── Email ── */}
      <Stack sx={{ gap: 1 }}>
        <SectionLabel>Email</SectionLabel>
        {contact.email ? (
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, p: 1.25, borderRadius: '10px', border: 1, borderColor: 'divider' }}>
            <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, minWidth: 0, fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' }} noWrap>
              {contact.email.address}
            </Typography>
            <Button size="small" onClick={() => void navigator.clipboard.writeText(contact.email!.address)}>
              Copy
            </Button>
            <Button size="small" color="error" onClick={() => setConfirm('email')}>
              Remove
            </Button>
          </Stack>
        ) : !providers.agentmail ? (
          <Stack sx={{ gap: 0.75 }}>
            <Typography variant="caption" color="text.secondary">
              Addresses come from{' '}
              <Link href="https://agentmail.to" target="_blank" rel="noreferrer">
                AgentMail
              </Link>
              , built for agents (free tier, no card). Paste its API key once; every agent can then have an address.
            </Typography>
            <Stack direction="row" sx={{ gap: 0.75 }}>
              <InputBase type="password" placeholder="AgentMail API key" value={mailKey} onChange={(e) => setMailKey(e.target.value)} sx={inputSx} />
              <Button
                size="small"
                variant="contained"
                disabled={!mailKey.trim() || !!busy}
                onClick={() => void run('mailkey', async () => {
                  await saveKey('agentmail', mailKey)
                  setMailKey('')
                  await refresh()
                })}
              >
                Save
              </Button>
            </Stack>
          </Stack>
        ) : (
          <Stack direction="row" sx={{ gap: 0.75, alignItems: 'center' }}>
            <InputBase
              placeholder={`${agent.id} (optional)`}
              value={username}
              onChange={(e) => setUsername(e.target.value.replace(/[^a-zA-Z0-9._-]/g, ''))}
              sx={inputSx}
              endAdornment={
                <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
                  @agentmail.to
                </Typography>
              }
            />
            <Button
              size="small"
              variant="contained"
              disabled={!!busy}
              onClick={() =>
                void run('email', async () => {
                  const c = await call<Contact>('cmd_contact_create_email', { ...args, username: username || agent.id, displayName: agent.name })
                  if (c) setContact(c)
                })
              }
            >
              {busy === 'email' ? 'Creating...' : 'Create address'}
            </Button>
          </Stack>
        )}
      </Stack>

      {/* ── Phone ── */}
      <Stack sx={{ gap: 1 }}>
        <SectionLabel>Phone</SectionLabel>
        {contact.phone ? (
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, p: 1.25, borderRadius: '10px', border: 1, borderColor: 'divider' }}>
            <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' }}>
              {contact.phone.number}
            </Typography>
            <Button size="small" color="error" onClick={() => setConfirm('phone')}>
              Release
            </Button>
          </Stack>
        ) : !providers.twilio ? (
          <Stack sx={{ gap: 0.75 }}>
            <Typography variant="caption" color="text.secondary">
              Numbers come from your{' '}
              <Link href="https://console.twilio.com" target="_blank" rel="noreferrer">
                Twilio
              </Link>{' '}
              account. Paste its Account SID and Auth Token once.
            </Typography>
            <Stack direction="row" sx={{ gap: 0.75 }}>
              <InputBase placeholder="Account SID" value={twilioSid} onChange={(e) => setTwilioSid(e.target.value)} sx={inputSx} />
              <InputBase type="password" placeholder="Auth Token" value={twilioToken} onChange={(e) => setTwilioToken(e.target.value)} sx={inputSx} />
            </Stack>
            <Button
              size="small"
              variant="contained"
              sx={{ alignSelf: 'flex-start' }}
              disabled={!twilioSid.trim() || !twilioToken.trim() || !!busy}
              onClick={() =>
                void run('twiliokey', async () => {
                  await saveKey('twilio-sid', twilioSid)
                  await saveKey('twilio-token', twilioToken)
                  setTwilioSid('')
                  setTwilioToken('')
                  await refresh()
                })
              }
            >
              Save
            </Button>
          </Stack>
        ) : (
          <Stack sx={{ gap: 0.75 }}>
            <Stack direction="row" sx={{ gap: 0.75 }}>
              <InputBase placeholder="Country (US)" value={country} onChange={(e) => setCountry(e.target.value.toUpperCase().slice(0, 2))} sx={{ ...inputSx, flex: '0 0 96px' }} />
              <InputBase placeholder="Area code (optional)" value={areaCode} onChange={(e) => setAreaCode(e.target.value.replace(/\D/g, ''))} sx={inputSx} />
              <Button
                size="small"
                variant="outlined"
                disabled={!!busy || country.length !== 2}
                onClick={() =>
                  void run('search', async () => {
                    setOffer((await call<NumberOffer>('cmd_contact_search_numbers', { country, areaCode: areaCode || null })) ?? null)
                  })
                }
              >
                {busy === 'search' ? 'Looking...' : 'Find numbers'}
              </Button>
            </Stack>
            {offer && (
              <Stack sx={{ gap: 0.25, border: 1, borderColor: 'divider', borderRadius: '10px', p: 0.5 }}>
                {offer.numbers.length === 0 && (
                  <Typography variant="caption" color="text.secondary" sx={{ p: 1 }}>
                    None available there. Try another area code.
                  </Typography>
                )}
                {offer.numbers.map((n) => (
                  <Stack key={n.number} direction="row" sx={{ alignItems: 'center', gap: 1, px: 1, py: 0.5, borderRadius: '8px', '&:hover': { bgcolor: 'action.hover' } }}>
                    <Typography variant="body2" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' }}>
                      {n.number}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" noWrap sx={{ flex: 1, minWidth: 0 }}>
                      {n.label}
                    </Typography>
                    <Button size="small" onClick={() => setBuying(n.number)}>
                      Buy
                    </Button>
                  </Stack>
                ))}
              </Stack>
            )}
            <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
              A number is billed to your Twilio account every month{offer?.monthlyPrice ? ` (${offer.monthlyPrice} for a local number here)` : ''}, plus each text
              and call. In the US, texting other US numbers needs the number registered for A2P 10DLC in the Twilio console; calls work straight
              away.
            </Typography>
          </Stack>
        )}
      </Stack>

      {/* ── Inbound ── */}
      {(contact.email || contact.phone) && (
        <Box sx={{ p: 1.25, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.04) }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <Stack sx={{ flex: 1 }}>
              <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
                Handle what comes in
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
                New email and texts go to {agent.name} as they arrive (checked every minute), shown in your chat with it. Anyone can write to
                an address, so it treats them as information, never as your instructions: what it does about them still follows its rules.
              </Typography>
            </Stack>
            <Switch
              checked={contact.inbound}
              onChange={(e) =>
                void run('inbound', async () => {
                  const c = await call<Contact>('cmd_contact_set_inbound', { ...args, on: e.target.checked })
                  if (c) setContact(c)
                })
              }
            />
          </Stack>
        </Box>
      )}

      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}

      <Dialog open={!!buying} onClose={() => busy !== 'buy' && setBuying(null)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>Buy {buying}?</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary">
            It's bought on your Twilio account and billed monthly{offer?.monthlyPrice ? ` (${offer.monthlyPrice})` : ''} until you release it, plus
            usage. It becomes {agent.name}'s number.
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button color="inherit" onClick={() => setBuying(null)} disabled={busy === 'buy'}>
            Cancel
          </Button>
          <Button
            variant="contained"
            disableElevation
            disabled={busy === 'buy'}
            onClick={() =>
              void run('buy', async () => {
                const c = await call<Contact>('cmd_contact_buy_number', { ...args, number: buying })
                if (c) setContact(c)
                setBuying(null)
                setOffer(null)
              })
            }
          >
            {busy === 'buy' ? 'Buying...' : 'Buy number'}
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog open={!!confirm} onClose={() => setConfirm(null)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>{confirm === 'email' ? 'Remove this address?' : 'Release this number?'}</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary">
            {confirm === 'email'
              ? `${contact.email?.address} and all its mail are deleted at AgentMail. The address can't be recovered.`
              : `${contact.phone?.number} goes back to Twilio and billing for it stops. You may not get the same number again.`}
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button color="inherit" onClick={() => setConfirm(null)}>
            Keep it
          </Button>
          <Button
            variant="contained"
            color="error"
            disableElevation
            disabled={!!busy}
            onClick={() =>
              void run('remove', async () => {
                const c = await call<Contact>(confirm === 'email' ? 'cmd_contact_remove_email' : 'cmd_contact_release_number', args)
                if (c) setContact(c)
                setConfirm(null)
              })
            }
          >
            {confirm === 'email' ? 'Remove' : 'Release'}
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  )
}
