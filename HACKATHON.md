# Vivianne: NVIDIA x Nebius edition

Submission notes for the Nebius x NVIDIA Global AI Hackathon.
**Track: Personal AI** (always-on, private assistant with persistent memory, reusable skills and tools you choose).

## One user, one problem

**User:** someone who gives an AI assistant real work (research, files, browsing, recurring checks) and does not want their notes, memory and sign-ins living in somebody else's product.

**Problem:** assistants that stay useful need memory, tools and time, and they get expensive and opaque fast. You can't see what each call cost, you can't tell what a skill is about to do, and everything runs on one big model.

**Answer:** Vivianne is a desktop assistant whose memory, skills and tools live on your machine, and whose thinking runs on NVIDIA Nemotron through Nebius Token Factory. A strong model plans and answers; a small one does the background work; every call is priced where you can see it.

## What is in the demo (and nothing else)

| Beat | What it shows | Where it is in the app | Judging criterion |
|---|---|---|---|
| Set up | Paste a Nebius key: Nemotron Ultra picked for chat, Nano for background work, routing on | First-run card (title-bar badge reopens it) | Design |
| Ask | A research question answered with Tavily search and a "Used N sources" list | Chat | Technological Implementation, Tavily bonus |
| Report | The reply as a summary card (table, finding cards, steps) | Chat and teammate chats | Design |
| Cost | Per-model usage and what routing saved against the big model | Settings, Usage by model; Providers, Cost-aware routing | Technological Implementation |
| Skills | Browse NVIDIA and Nebius skills, review the scan and the pinned manifest, install, `@mention` | Settings, Skills, Browse | Quality of the Idea |
| Memory | What it remembers is yours: list, edit, delete; embedded with a Nemotron model | Memory panel, Settings, Memory | Potential Impact |
| Always on | A teammate with a routine that posts a card while you are away | Team, routines | Potential Impact |

Out of scope for the demo (present in the app, not part of the submission): the notch, voice, computer control, mods, the timeline, workflows, local models. Don't open them on camera.

## Three-minute script

1. **0:00** The problem in one sentence over the empty app.
2. **0:10** First-run card: paste the key, show the two models chosen, press Use these.
3. **0:35** Ask a current-events question. Show it searching, then the reply card and the sources.
4. **1:15** Open Usage by model: Ultra and Nano side by side, tokens and cost, and the routing saving.
5. **1:45** Skills: install one NVIDIA skill, show the review sheet (findings, files, pinned commit), then use it.
6. **2:15** Memory: show what it remembered from earlier, delete one entry, and name the embedder.
7. **2:35** Teammate: open its routine, run it now, show the card it posts.
8. **2:55** One closing line: what runs where.

Record on the machine the judges would use. No third-party music.

## What is new since Aug 26, 2026

Vivianne existed before the hackathon. Significantly updated during the submission period:

- Nebius Token Factory provider, with model discovery and per-model cost accounting
- Cost-aware routing (beta): background work on a small Nemotron model, with a savings meter
- NVIDIA and Nebius skills hub: scan, skill card view, commit-pinned manifest, modified-since-install warning
- Selectable cloud embedder for memory (Nebius Qwen3 or an NVIDIA Nemotron embed model)
- Tavily search with a test button and a sources list under replies
- Report-shaped replies rendered as summary cards; teammates report in cards
- Agent faces that show live state; the browser tool for any Chromium browser
- Programmatic tool calling made always-on, with `toolInfo` and `parallel`
- The NVIDIA x Nebius edition: themes, quickstart, edition badge

## Platform support

| | Windows | macOS | Linux |
|---|---|---|---|
| App, chat, memory, skills, routing, embedder | Verified | Builds in CI; not exercised by hand | Builds; not exercised by hand |
| Teammates, routines, browser extension | Verified | Same code, not exercised | Same code, not exercised |
| Computer control (DIP), notch, dictation chord | Verified | Not available | Not available |

If you are judging on macOS or Linux: everything in the demo above is in the portable part. Computer control is the only thing you will not see.

## Needs a key to verify

These were built against documented APIs and fixtures and have not been run against a live Nebius or NVIDIA account:

- Listing and calling Nemotron models on Token Factory (exact model ids come from the account)
- Per-model prices for Nemotron models (shown as "unpriced" until the provider reports them)
- The NVIDIA Nemotron embedder (needs an NVIDIA key; Nebius serves Qwen3 for embeddings, not a Nemotron embedder)
- Streaming usage totals from Token Factory

## Honest limits

- Skills are scanned by heuristics, and NVIDIA's signatures are not verified in the app.
- Routines run while the app is running (it keeps going in the tray). Heavy jobs can go to Nebius Serverless Jobs through `nebius_job`; nothing else is hosted.
- There is no OS-level sandbox; actions go through approvals.
