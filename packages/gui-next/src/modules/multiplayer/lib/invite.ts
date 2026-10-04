/**
 * Pairing invites: one string carrying a pairing code and a name, so the
 * person you invite pastes a single thing and sees who it's from.
 *
 * `vivianne-invite:<base64url JSON {v, id, name}>`. A bare pairing code is
 * still accepted, for codes shared before invites existed.
 */

const PREFIX = 'vivianne-invite:'

export interface Invite {
  id: string
  name: string | null
}

function b64url(text: string): string {
  return btoa(String.fromCharCode(...new TextEncoder().encode(text)))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
}

function unb64url(text: string): string {
  const b64 = text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (text.length % 4)) % 4)
  return new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)))
}

export function makeInvite(id: string, name: string | null): string {
  return PREFIX + b64url(JSON.stringify({ v: 1, id, name: name?.trim() || null }))
}

/** An invite or a bare pairing code → who to pair with; null if neither. */
export function parseInvite(text: string): Invite | null {
  const t = text.trim()
  const at = t.indexOf(PREFIX)
  if (at >= 0) {
    try {
      const body = t.slice(at + PREFIX.length).split(/\s/)[0]
      const v = JSON.parse(unb64url(body)) as { id?: unknown; name?: unknown }
      if (typeof v.id === 'string' && v.id.length >= 16) return { id: v.id, name: typeof v.name === 'string' ? v.name : null }
    } catch {
      return null
    }
    return null
  }
  // A bare node id: long, no spaces, letters and digits.
  return /^[a-z0-9]{40,80}$/i.test(t) ? { id: t, name: null } : null
}

/** A readable share message with the invite in it. */
export function inviteMessage(id: string, name: string | null): string {
  return `${name?.trim() || 'Someone'} invited you to pair in Vivianne. In Vivianne, start a new project, choose Multiplayer project, and paste this where it asks for their invite:\n\n${makeInvite(id, name)}`
}
