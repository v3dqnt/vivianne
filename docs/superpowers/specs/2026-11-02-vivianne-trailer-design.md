# Vivianne trailer (HyperFrames) — design

## Goal
A product trailer for Vivianne, built with HyperFrames (HTML to MP4), showing the
24/7 agents, code mode, and multiplayer. The trailer is **open-ended**: the
current cut stops after the multiplayer scene, but more scenes will be added
later, so there is no end card and adding a scene must be a one-file change.

## Format
- Project: `packages/trailer-hyperframes` (the Remotion `packages/trailer` is untouched).
- 1920x1080, 30fps, about 56s for the current cut, rendered to `out/vivianne-trailer.mp4`.
- Root `index.html` sequences one sub-composition per scene. A scene is
  registered in a single scene list (file, start, duration); adding one means
  adding a scene file and one list entry.

## Scenes (current cut)
| # | Scene | ~Time | Content |
|---|---|---|---|
| 1 | s1-intro | 0-7s | City b-roll slot + "Introducing Vivianne" |
| 2 | s2-laptop | 7-13s | Laptop-opening slot, push-in dissolving into the app |
| 3 | s3-agents | 13-30s | 24/7 agents chat: email + Slack briefing, WhatsApp flag from wife, grocery list |
| 4 | s4-title-work | 30-34s | "Now let's get to work, shall we?" |
| 5 | s5-code | 34-52s | Code mode: new thread, Issues tab, pick website issue, prompt, pick model, send, sub-agents survey and fix |
| 6 | s6-title-buddies | 52-56s | "Wanna destroy some bugs with your buddies?" |
| 7 | s7-multiplayer | 56-70s | Invite link, friend joins, both prompt Vivianne. Cut ends here; no end card. |

Times are targets; the scene list is the source of truth.

## Footage slots
Scenes 1 and 2 each contain a video slot (`assets/footage/city.mp4`,
`assets/footage/laptop.mp4`). When the file is absent a generated placeholder
renders (night skyline with light-trails; laptop silhouette with an illustrated
figure). Dropping the file in replaces the placeholder with no code change.
Text and the push-in transition sit above the slot.

## UI recreation
Static HTML/CSS ports of the real components in `packages/gui-next/src`
(`modules/team` BotChat/BotSteps/BotPage, `modules/sessions` Sidebar/ProjectRail,
the code-mode issues tab, `ModelPicker`, `modules/multiplayer` PairProjectDialog/CollabView).
Layout, spacing, radii and copy are copied from source; colours are CSS variables
generated from `theme.ts` tokens (dark, monochrome default). Markup and timeline
are kept separate per screen.

## Story data
One data file holds all invented demo content: the briefing, the five-item
grocery list, the website issue, the sub-agent steps, chat lines. Copy edits
never touch markup.

## Audio
Reuse `theme.mp3` and `beatgrid.json` from `packages/trailer` to time cuts
(copied into the new package's assets, not referenced across packages).

## Verification
`hyperframes lint`; step each scene in preview; screenshot comparison against the
real UI; render the MP4 and check duration, resolution and sampled frames.

## Defaults chosen
Dark monochrome theme; placeholder silhouette for the man; the friend is a second
cursor and avatar in the collab view; README explains footage swap, adding scenes
and rendering.

## Out of scope
Real footage, voiceover, light theme variants, running the actual React app.
