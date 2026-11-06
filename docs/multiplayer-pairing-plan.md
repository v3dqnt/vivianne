# Multiplayer pairing — making it usable end to end

Companion to `docs/multiplayer-update-plan.md`. That document covers what shared
work *is* (transport, activity log, usage, git sync) and it is built. This one
covers only the path a person walks to get there, which today asks too much of
both people and fails in ways that read as network errors.

## 1. What is wrong today

Pairing two real people, as implemented:

1. **A** opens *New project → Multiplayer*, types a name, copies an invite
   (node id + display name). No project exists on A's machine yet.
2. A sends it out of band.
3. **B** opens the same dialog, copies *their* invite, sends it back.
4. A pastes B's invite, types the project name, picks a folder, creates.
5. B pastes A's invite, retypes the **same** project name, picks *their* folder,
   creates.
6. Whoever presses Connect first may be refused: `is_authorized_peer` only
   accepts a peer that some project already names, and the refusal surfaces as
   `Couldn't reach that peer: …`.

Five gaps:

| # | Gap | What the user experiences |
|---|---|---|
| 1 | The invite carries a node id and a name, not the project | The invitee invents a project name and folder from nothing |
| 2 | Authorization is mutual; the flow presents it as one-sided | Both people must send *and* paste an invite, and nobody is told |
| 3 | First dial is refused until both projects exist | A network-sounding error that names the wrong cause |
| 4 | No reconnect | Every launch and every blip needs a manual Connect click |
| 5 | No way out | No remove-partner, cancel-invite, or revoke |

## 2. Decisions

1. **An invite is a single-use join link bound to one project.** Holding a live
   token *is* the authorization; it is burned on first successful use.
2. **The joining side is zero-setup.** The link carries the project name and the
   inviter's identity; the joiner chooses a folder and nothing else. The folder
   is created, git-initialised, and filled from the inviter's first sync.
3. **Authorization moves after the `Hello` exchange.** The accept loop reads one
   small frame before deciding, because only then does it know which project (if
   any) the token belongs to.
4. **Auto-reconnect** for the active multiplayer project, with backoff.
5. **Removal is symmetric** and is announced over the wire, so the other side
   stops dialling instead of retrying forever.
6. **Protocol v3.** `Hello` gains an optional token; ALPN moves to
   `vivianne/multiplayer/3` so a mixed pair fails with the existing
   "incompatible version" message rather than a decode error.

## 3. Invite link

```
vivianne-join:<base64url { v: 2, peer, name, project, token }>
```

- `peer` — the inviter's iroh node id (64 hex/base32 chars, as `EndpointId`
  prints today).
- `name` — the inviter's display name, shown as "@Dana invited you".
- `project` — the project name, prefilled on the joining side.
- `token` — 32 random bytes, base32 without padding (~52 chars).

Server side, `MultiplayerProjectInfo` stores a digest, never the token:

```rust
pub struct PendingInvite {
    /// sha256 of the token in the link.
    pub token_hash: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub state: InviteState,        // pending | joined
}
```

Rules:

- Minting again replaces the previous invite (old link dies).
- Revoking clears it.
- Expiry: 7 days.
- A used invite sets `state: joined` and clears `token_hash` — a replay finds
  nothing to match.

`parseInvite` in `packages/gui-next/src/modules/multiplayer/lib/invite.ts` gains
the join shape alongside the existing `vivianne-invite:` one, so links already
sent keep working.

## 4. Handshake

### Wire

```rust
Hello {
    protocol_version: u32,
    display_name: Option<String>,
    /// Present when this connection is redeeming an invite (first join only).
    invite_token: Option<String>,
}
```

### Accept path (`multiplayer/state.rs::run_accept_loop`)

Today: authorize by peer id **before** opening a stream, close otherwise.

New order:

1. Accept the connection, open the bi-di stream.
2. Read one `Hello` with a **5 s timeout** and a **4 KB** frame cap (the 4 MB
   data cap is not appropriate for a pre-auth frame).
3. If `invite_token` is present, hash it and look for a `pending`, unexpired
   invite across all projects (constant-time compare).
   - Match → bind `peer_id` + `peer_display_name` into that project, clear the
     token, persist `projects.json`, emit `multiplayer_joined` and the new
     `multiplayer_status`, then continue through the normal `adopt_connection`
     path.
   - No match → close with `b"invalid invite"`. No project data has been sent.
4. No token → the existing `is_authorized_peer` gate, unchanged.

Pre-auth surface: exactly one frame, bounded, timed out. Nothing else is read or
written before a decision. A per-peer counter (5 failed joins per 10 minutes)
closes the connection with no reply beyond the first attempt; at 256 bits the
token is not guessable, so this only bounds noise.

### Inviter (`cmd_create_shared_project`)

Creates the project, ensures a git repo **with one commit** (today only the
test-partner path does that — `collab::commit_all` is already there to reuse),
mints the invite, returns `{ project, invite }`.

The UI shows the link with Copy and Revoke, and the state line
`Invited · waiting for Dana`.

### Joiner (`cmd_join_shared_project`)

`{ invite, path }`:

1. Refuse a folder that is already a project on this machine (any mode) — it
   belongs to something else already.
2. `create_dir_all`, then `git init` if there's no repo.
3. If the folder already has files: stage and commit them as
   `local files before joining`, so the inviter's history merges onto a real
   base and conflicts surface through the existing sync-note path instead of a
   raw merge failure.
4. Write the project record with `peer_id = inviter` and `peer_display_name`.
   No invite is stored on this side — it has already been spent.
5. Dial the inviter with `invite_token`.

### First sync, unborn HEAD

The inviter's `flush` sees `peer_head = None` and bundles the whole history. The
joiner fetches it into `refs/viv/partner`, and `merge_partner` hits a case the
test-partner flow never does — it *clones*, so it always has a HEAD. A repo with
no commits cannot `git merge`:

```rust
if git(dir, &["rev-parse", "--verify", "HEAD"]).await.is_err() {
    // First join: adopt the partner's history wholesale. Nothing local to
    // lose — §4 refuses or commits any pre-existing files first.
    let branch = git(dir, &["symbolic-ref", "--short", "HEAD"]).await
        .unwrap_or_else(|_| "main".into());
    git(dir, &["checkout", "-B", &branch, "refs/viv/partner"]).await?;
} else {
    /* existing merge path */
}
```

Covered by extending `bundles_carry_commits_between_copies` in
`multiplayer/collab.rs` with a third repo that starts empty.

Known limit, unchanged: a first bundle over `MAX_BUNDLE_BYTES` (2.9 MB of frame)
cannot be sent directly. The existing note becomes actionable — it offers the
shared-remote field rather than only explaining the problem.

## 5. States

| Condition | Panel header | Action offered |
|---|---|---|
| Invite minted, no peer bound | `Invited · waiting for Dana` | Copy link, Revoke |
| Dialling | `Connecting…` | Cancel |
| Connected | `Online` / `Dana's agent is working…` | Disconnect |
| Dropped | `Offline · retrying in 8s` | Retry now |
| Refused by token | `That invite is no longer valid` | Ask for a new link |
| Partner removed | `Dana removed this project` | Invite someone else |

### Auto-reconnect

A supervisor task in `multiplayer/`, driven by status changes:

- target = the peer of the multiplayer project matching the active workspace
  (`find_project_for_workspace`), or the project last connected;
- sequence 1 s, 2 s, 5 s, 15 s, 30 s, 60 s (capped, with ±20 % jitter);
- stops on success, on removal, when the app is offline, or when the user
  disconnects deliberately — a `user_disconnected: AtomicBool` on
  `MultiplayerState`, set by `cmd_multiplayer_disconnect`, cleared when a
  multiplayer project opens or Connect is pressed;
- restarts on launch.

One connection at a time, as `MultiplayerState` already holds.

### Removal

`cmd_remove_partner { projectId }`:

1. Send `WireMessage::PartnerRemoved`, close the connection.
2. Clear `peer_id`, `peer_display_name`, and any invite.
3. The project stays, as a solo project with a "share this project" affordance.

Receiver: clear the same fields, mark the project `partner_removed` so the
header explains why, stop retrying.

## 6. Data model

`MultiplayerProjectInfo.peer_id: String` → `Option<String>`, plus
`invite: Option<PendingInvite>` and `partner_removed: bool` (defaulted).

Ripples, all small:

- `projects.rs::is_authorized_peer` — `is_some_and(|id| id == peer_id)`.
- `multiplayer/collab.rs::project_for_peer` — same.
- `multiplayer/mod.rs::authorized_peer_for_workspace` — same.
- `PairChip.tsx`, `MultiplayerWorkspacePanel.tsx` presence derivation — a
  peerless project is `invited`, not `offline`.
- `cmd_create_multiplayer_project` keeps its shape for the test-partner mirror;
  the UI path moves to `cmd_create_shared_project` / `cmd_join_shared_project`.

## 7. Files

**Backend** (`packages/gui/src-tauri/src/`)

- `multiplayer/protocol.rs` — v3, `Hello.invite_token`, `PartnerRemoved`.
- `multiplayer/state.rs` — accept-after-Hello, token redemption, removal frame,
  reconnect supervisor.
- `multiplayer/commands.rs` — `cmd_create_shared_project`,
  `cmd_join_shared_project`, `cmd_multiplayer_mint_invite`,
  `cmd_multiplayer_revoke_invite`, `cmd_multiplayer_remove_partner`.
- `multiplayer/collab.rs` — unborn-HEAD merge, actionable oversized-bundle note.
- `agent/sessions/projects.rs` — `Option<peer_id>`, `PendingInvite`, binding.

**UI** (`packages/gui-next/src/modules/multiplayer/`)

- `lib/invite.ts` — `makeJoinLink` / `parseJoinLink`, keep the old parser.
- `components/PairProjectDialog.tsx` → split into `InviteDialog` (invite
  someone) and `JoinDialog` (paste a link); the new-project flow offers the two.
- `components/MultiplayerWorkspacePanel.tsx` — invited/waiting/retrying states,
  remove partner, revoke.
- `store/multiplayerStore.ts` — `multiplayer_joined` listener, retry countdown,
  `removePartner`, `mintInvite`, `revokeInvite`.

## 8. Build order

1. **Protocol v3 + token redemption.** Wire change, accept-after-Hello, digest
   storage, binding. Tests: valid token binds and burns; wrong/expired/revoked
   rejected; replay rejected; tokenless unknown peer still rejected.
2. **Zero-setup join.** `cmd_join_shared_project`, initial commit on create,
   unborn-HEAD merge. Test: joins an empty folder and ends on the inviter's
   HEAD; a non-empty folder merges with the local commit as the base.
3. **Dialogs and states.** Invite / Join, waiting, joined, revoke.
4. **Removal.** Command, wire frame, both sides clearing.
5. **Auto-reconnect.** Supervisor, backoff, launch restore.
6. **Docs.** Update `multiplayer-update-plan.md`'s phase list; note that
   existing multiplayer projects keep their `peer_id` and need no migration
   beyond the defaulted fields.

## 9. Out of scope

- More than two peers per project.
- Same-file live co-editing (unchanged stretch goal).
- Approval prompts for joining: the token *is* the approval.
- Hosted matchmaking or accounts: the link is exchanged out of band, as today.

## 10. Verification

`packages/gui-next` has no test runner, so UI is verified by hand:

- `cargo test` for the backend suites above.
- Two instances on one machine via the existing test partner, plus a real
  second machine where possible: invite → paste → join → chat → activity →
  first sync → restart both apps (auto-reconnect) → remove partner.
