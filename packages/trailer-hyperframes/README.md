# Vivianne trailer (HyperFrames)

The Vivianne product trailer, written as HTML and rendered to MP4 with
[HyperFrames](https://hyperframes.heygen.com). 1920×1080, 30fps, about 70s.

| # | Scene | File | What happens |
|---|---|---|---|
| 1 | Intro | `compositions/s1-intro.html` | "Introducing Vivianne" over city b-roll (placeholder slate) |
| 2 | Laptop | `compositions/s2-laptop.html` | Laptop shot (placeholder slate); the app zooms up out of it |
| 3 | 24/7 agents | `compositions/s3-agents.html` | Ava, an always-on agent, briefs Sam on email, Slack and WhatsApp, and makes Maya's grocery list |
| 4 | Title | `compositions/title-card.html` | "Now let's get to work, shall we?" |
| 5 | Code mode | `compositions/s5-code.html` | New thread → Issues → #142 → prompt → Opus → send; subagents fix it |
| 6 | Title | `compositions/title-card.html` | "Wanna destroy some bugs with your buddies?" |
| 7 | Multiplayer | `compositions/s7-multiplayer.html` | Sam invites Leo, Leo joins, both prompt Vivianne |

The cut ends after scene 7 on purpose: there is no end card, so more scenes
can follow.

## Commands

```bash
npm run build      # scenes.json → index.html (run after any change to scenes.json)
npm test           # build-script and helper tests
npm run check      # HyperFrames lint, layout, motion and contrast checks
npm run dev        # preview in the browser
npm run render     # → out/vivianne-trailer.mp4
```

## Swapping in real footage

Scenes 1 and 2 have footage slots. Until a clip exists they show a labelled
placeholder slate. Drop in:

- `assets/footage/city.mp4`: busy city b-roll, under "Introducing Vivianne"
- `assets/footage/laptop.mp4`: a person opening their laptop

then run `npm run build`. The clip is trimmed to its scene, plays muted, and
the slate hides itself. The title and the zoom into the app play over it.

## Adding a scene

1. Add `compositions/<id>.html`. Copy the shape of an existing scene: a
   `<template>`, a root `data-composition-id="<id>"`, and one paused GSAP
   timeline registered on `window.__timelines["<id>"]`.
2. Add `{ "id": "<id>", "src": "compositions/<id>.html", "duration": <s> }`
   to `scenes.json`, in order. Start times are worked out from durations.
3. `npm run build`.

Each scene receives `duration` (and `hasFootage`) through
`window.__hyperframes.getVariables()`. A second title card is just another
entry pointing at `compositions/title-card.html` with `"variables": { "line": "…" }`.

## Changing the words

Every line of demo copy lives in `assets/story.js`: the briefing, the grocery
list, the issue, the prompts, the collaboration lines. The app UI in scenes 3,
5 and 7 is captured with that copy baked in, so after editing it re-run the
captures (below).

## The app UI is the real app

Scenes 3, 5 and 7 don't imitate Vivianne. They show the real GUI
(`packages/gui-next`) running on a fake backend with demo data, captured as
markup and CSS (`captures/*.js`) and then animated. To re-capture after the
app or the copy changes:

```bash
npm install                               # once, from the repo root (gui-next deps + playwright)
npx playwright install chromium           # once, if this machine has no Playwright browser
(cd ../gui-next && npx vite --port 5199)  # in another terminal
npm run capture -- agents                 # also: code, multiplayer, chrome
npm run build
```

`tools/capture/` has the details:

- `mock-ipc.js` is the fake Tauri bridge.
- `demo-base.js` holds the shared demo data.
- `scenarios/*.cjs` drive each scene's states.

Reference screenshots of every state land in `captures/ref/` (git-ignored).
Colours for everything else come from the app's theme via `npm run tokens`.

Two things in the captures are not in the app as it stands:

- **The Issues tab** (scene 5) doesn't exist in the app yet; GitHub issues are
  agent tools only. `tools/capture/scenarios/issues.js` adds it to the real
  dock in the app's style.
- **Two layout bugs** are smoothed over in the capture only:
  - The model menu's provider headers stick 15px too low, over each group's
    first model (`ModelSelector.tsx`, `STICKY_HEADER_H = 84` while the search
    block is 69px).
  - The floating plan pill covers the first user message in a short chat.

## Music

`assets/audio/theme-cut.mp3` is 70s of `packages/trailer/public/audio/theme.mp3`
taken from 0:38, with a 0.8s fade-in. Cuts land on onsets. The quiet
breakdown carries the first title card, and the beat drops back exactly as
Sam hits send in code mode (41.9s).
