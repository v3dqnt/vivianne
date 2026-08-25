# The Multiplayer Update — architecture plan

Two people, two local Vivianne instances, one shared session: merged usage, pooled skills/context, a shared workspace, a chat panel between the humans, and the agents able to use that same channel to coordinate and split work. This document is the technical plan — libraries, architecture, and a phased build order.

## 1. Why this is hard (and what makes it tractable)

Every "multiplayer" building block already sounds solved in isolation (chat, file sync, presence). The actual hard problem is that **five different pieces of state need to stay consistent across two independent processes, on two machines, over an unreliable connection**:

1. The shared agent transcript (both agents need to know what the other did)
2. The workspace files (both users editing the same project)
3. Usage/cost accounting (merged view, still attributed)
4. The skill/context catalog (union of both users' locally-installed skills)
5. The human + agent chat log

Trying to hand-roll conflict resolution for all five is how this kind of feature turns into a year-long distributed-systems project. The move is: **pick one battle-tested sync primitive and make everything above ride on it**, rather than inventing five bespoke sync protocols.

## 2. Core architecture

```
┌─────────────────────────┐         ┌─────────────────────────┐
│   Vivianne (User A)      │         │   Vivianne (User B)      │
│                          │         │                          │
│  Tauri/Rust backend      │  iroh   │  Tauri/Rust backend      │
│  ┌────────────────────┐  │◄───────►│  ┌────────────────────┐  │
│  │ yrs::Doc (shared)   │  │  QUIC   │  │ yrs::Doc (shared)   │  │
│  │  - transcript log   │  │  P2P    │  │  - transcript log   │  │
│  │  - chat log         │  │         │  │  - chat log         │  │
│  │  - usage map        │  │         │  │  - usage map        │  │
│  │  - skill catalog    │  │         │  │  - skill catalog    │  │
│  │  - presence/locks   │  │         │  │  - presence/locks   │  │
│  └────────────────────┘  │         │  └────────────────────┘  │
│  AgentState / AppState   │         │  AgentState / AppState   │
└─────────────────────────┘         └─────────────────────────┘
         │                                        │
         └──────────────► shared git remote ◄─────┘
                        (workspace sync)
```

Both instances hold a **local replica of the same CRDT document**. There is no server in the middle for the core sync — it's direct P2P. Each side keeps working offline-tolerant; when the connection blips, both replicas just resync and converge automatically. This is the actual value CRDTs buy you here: neither agent has to "wait for the network" to keep working.

### 2.1 Transport: `iroh`

**Recommendation: [iroh](https://github.com/n0-computer/iroh) (Rust, QUIC-based, hit 1.0/production-ready).**

Why this over the alternatives:
- Vivianne's backend is already Rust — iroh runs *in-process* in `src-tauri`, no separate sidecar, no JS networking layer.
- Handles NAT traversal and peer discovery itself (via its relay network, same idea as Tailscale's DERP) — you don't stand up your own signaling server for "two home users behind routers" to connect.
- QUIC gives you encrypted, multiplexed streams for free — one connection carries the CRDT sync stream, the chat stream, and file-transfer traffic without you building your own framing.
- `matchbox`/`webrtc-rs` are the alternative — skip them. They're WASM/browser-first (matchbox is literally built for browser game netcode); iroh is the one built for exactly this "two native apps talk directly" case.

**Pairing UX**: iroh gives each node a stable node ID. The "connect your Vivianne to a friend's" flow is a short pairing code/link exchanged out-of-band (same pattern as Tailscale, Syncthing, or a game's "invite code") — no account system or hosted matchmaking service required for v1.

### 2.2 State sync: `yrs`

**Recommendation: [yrs](https://github.com/y-crdt/y-crdt) — the Rust-native, binary-compatible port of Yjs.**

Why this over Automerge/Loro/raw Yjs:
- It's Rust. Vivianne's state (`AgentState`, `AppState`, usage store, session files) already lives in Rust — `yrs` lets the CRDT doc live in the same process, wired directly to existing state, instead of running a whole separate JS/Node Yjs sync server alongside a Rust backend (two runtimes, two deploy targets, for no reason).
- Yjs is the proven, fast, small-footprint choice for this class of problem (10-50x faster than Automerge on large docs in current benchmarks) and `yrs` tracks it closely enough to be wire-compatible — if a web/mobile companion client ever gets built, it can talk to the exact same doc format.
- Loro is real competition on raw performance and has a nicer Rust API, but it's the newer, thinner-ecosystem option — for a v1 with real deadlines, `yrs`'s maturity wins. Worth revisiting if Loro's ecosystem catches up.

**Document shape** (one `yrs::Doc` per paired session):
- `Y.Array<TranscriptEvent>` — the shared project activity log (see §3)
- `Y.Array<ChatMessage>` — human + agent chat, one stream
- `Y.Map<String, DayUsage>` — usage keyed by `peer_id:day`, merged for display
- `Y.Map<String, SkillMeta>` — the union catalog of both peers' installed skills (metadata only; content fetched on demand over the same iroh connection)
- `Y.Map<String, FileLock>` — soft presence locks ("User B is editing `auth.rs`")

### 2.3 Workspace sync: lean on git, don't reinvent it

Full real-time co-editing of the *same file* (Google-Docs style, CRDT-per-file) is the hardest, highest-risk part of this feature, and it's not what was asked for — the ask is "shared workspace + know what the other did," not "type in the same file simultaneously."

**v1 approach**: both peers work against the same git remote/branch. Vivianne already has deep git integration (`cmd_git_*` — status, diff, branches, checkout). Add an auto-sync loop: commit + push after each completed turn, pull before starting one, surface merge conflicts through the existing Review panel instead of a new UI. Presence locks (`Y.Map<String, FileLock>`) give a soft "heads up, they're touching this file right now" warning before a collision happens, rather than silently resolving it after.

This reuses machinery that already exists and is already trusted (git's merge model), instead of shipping a bespoke file-CRDT system as a v1 dependency. True same-file live co-editing is a legitimate phase-2/stretch goal once the rest of this is solid — treat it as optional, not core.

## 3. Shared agent context — the actual point of the feature

This is the part that differentiates it from "we added a chat sidebar." The mechanism: a shared, append-only **activity log** (`Y.Array<TranscriptEvent>`), where each entry is a structured summary — not the raw transcript — of what one agent did: file edited (path + diff summary), command run + result, goal/todo changed, decision made.

Each side's `Agent` already rebuilds its system prompt every turn from named blocks (`directive.rs::assemble_capability_blocks` — capability directive, mode framing, skills catalog, project instructions, memory). This slots in as one more block:

```
<vivianne_pair_activity>
Your pair (User B) in the last few minutes:
- edited packages/api/auth.rs (added token refresh logic)
- ran `cargo test` — 12 passed, 1 failed (test_expired_token)
- goal updated: "Ship auth refactor" → in progress
</vivianne_pair_activity>
```

This is *state awareness*, not transcript merging — each agent keeps its own primary conversation with its own user, but both always know the other's recent activity, budgeted the same way the existing skills/memory blocks are (summarized, not dumped verbatim, so it doesn't blow the context window). Compaction (`maybe_compact_context`) needs to know about this block too, same as it already handles the other injected blocks.

**Merged usage** rides the same pattern `usage_store.rs` already uses (`source_tag` = `"main:<session_id>"` today) — extend the tag with a peer id, sync deltas through the `Y.Map`, and the usage dashboard sums across peers while still being able to break out "who spent what."

## 4. Chat panel

A new dock panel, same shape as the existing `SidePanel`/`BottomTerminal` (`packages/gui-next/src/modules/dock/`). Message list backed by the `Y.Array<ChatMessage>` — reuse the existing `Markdown.tsx`/`DiffView.tsx` components for rendering code snippets and diffs in chat rather than building new renderers.

**Agents talking through the same channel**: give the agent a `send_to_partner` tool that appends an agent-authored message into the *same* shared stream (tagged `sender: "agent:A"`), visible to both humans — this is deliberately not a hidden side-channel. The other agent picks up new pair-chat messages the same way it'd notice a new user message, so "divide the work between them" happens as an ordinary tool call + the other agent reacting to what appears in its own feed, not a separate protocol.

## 5. On Cordis — what to actually take from it

[Cordis](https://github.com/cordiverse/cordis) (the plugin framework behind Koishi/the DeepSeek-harness plugin system) is genuinely well-designed: a `Context` tree where services attach as properties, demand-driven DI (`ctx.provide`/`ctx.inject` — a plugin's body only runs once its declared dependencies exist), and a fork/dispose lifecycle where killing a plugin automatically unwinds *everything* it registered — listeners, sub-services, commands — via runtime effect tracking.

**The honest take: most of what makes Cordis's implementation complex is Cordis solving a problem Rust doesn't have.** JS has no deterministic destructors, so Cordis has to *simulate* "clean up everything this scope touched" at runtime with an effect-tracking/fiber system. Rust gets that for free from `Drop` + RAII. A Rust-native version that copies Cordis's full reactivity model would be reimplementing something the language already gives you — that's not "lighter," that's redundant.

**What's actually worth taking**: the *shape* of the idea — a typed, scoped service registry or fine-grained plugin hot-reload — is genuinely useful, and this feature is a natural first real consumer of it (usage tracking, context injection, the tool registry, and the workspace all need to become "pairing-aware" simultaneously). A right-sized Rust version:

- A typed service registry on `AppState`/`AgentState` (`HashMap<TypeId, Box<dyn Any>>` behind a small typed `provide`/`inject` API — Rust's real answer to Cordis's context properties)
- Explicit dispose closures per registered resource, not automatic effect tracking — lean on `Drop`/RAII for the common case, and only reach for an explicit disposer where a resource crosses an async boundary Rust's ownership can't track alone (a spawned task, a registered event listener)
- No decorators, no metadata, no fiber system — a plugin is just a function that takes `&mut Context` and returns a disposer

This is smaller than Cordis by design, not by omission — it drops the machinery that exists to work around a limitation Rust doesn't have, and keeps the part (scoped, demand-driven services with clean teardown) that's actually the good idea.

## 6. Library shortlist

| Purpose | Pick | Why |
|---|---|---|
| P2P transport | [`iroh`](https://github.com/n0-computer/iroh) | Rust-native, QUIC, NAT traversal + peer discovery built in, 1.0/production |
| CRDT sync | [`yrs`](https://github.com/y-crdt/y-crdt) | Rust-native Yjs port, wire-compatible, proven performance |
| Workspace sync | Git (already integrated) | Reuse trusted merge semantics instead of a bespoke file-CRDT |
| Chat/diff rendering | Existing `Markdown.tsx` / `DiffView.tsx` | Already built, already themed |
| Plugin/service pattern | Small custom Rust registry, Cordis-*inspired* | Cordis's reactivity system solves a JS-only problem; RAII covers most of it for free |

Explicitly **not** using: a hosted realtime backend (PartyKit, Liveblocks, ElectricSQL) — those solve "many clients, one server, one source of truth," which isn't this problem (exactly two peers, no reason to route through a third party or pay for hosting); WebRTC directly (`matchbox`/`webrtc-rs`) — browser/game-netcode oriented, iroh is the native-app-to-native-app equivalent and is simpler to integrate from Rust.

## 7. Phased build order

1. **Transport + presence** — iroh pairing flow, connection status in the UI, no shared state yet beyond "who's online."
2. **Chat panel** — `Y.Array<ChatMessage>` over the iroh link, code-snippet rendering. Ships value on its own before anything else is built.
3. **Merged usage** — extend `usage_store`'s `source_tag`, sync deltas, merged dashboard view. Self-contained, low risk.
4. **Shared skill/context catalog** — union of installed skills, on-demand content fetch.
5. **Activity log + system-prompt injection** — the `<vivianne_pair_activity>` block, compaction awareness. This is the feature's actual centerpiece; do it after 1-4 are solid so the plumbing underneath is already proven.
6. **`send_to_partner` tool** — agent-to-agent coordination over the chat stream.
7. **Git-based workspace auto-sync** — commit/push/pull loop, presence-lock warnings, conflicts surfaced through the existing Review panel.
8. *(Stretch, later)* same-file live co-editing via per-file CRDTs, only if 1-7 prove the pattern is worth the added complexity.
