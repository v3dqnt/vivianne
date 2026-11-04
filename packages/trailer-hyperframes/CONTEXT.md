# Context for the next session: Vivianne trailer

Handoff from the cloud session that built this trailer. Read this file and
`README.md` (same folder) before changing anything.

## Where things are

- Repo: `v3dqnt/vivianne`. Branch: **`claude/vivianne-product-trailer-kgph3t`**.
  Run `git fetch && git checkout claude/vivianne-product-trailer-kgph3t && git pull`.
  Nothing is merged to `main` yet.
- Package: `packages/trailer-hyperframes`. A HyperFrames project: HTML and GSAP
  scenes rendered to MP4 by the `hyperframes@0.9.2` CLI (pinned in `package.json`).
- Spec: `docs/superpowers/specs/2026-11-02-vivianne-trailer-design.md`.
  Plan: `docs/superpowers/plans/2026-11-02-vivianne-trailer.md`.
- The older Remotion trailer, `packages/trailer`, is a separate project. Leave it alone.

## The cut (70s, 1920×1080, 30fps, with music)

| Start | Scene | File |
|---|---|---|
| 0.000 | "Introducing Vivianne" over **city b-roll** | `compositions/s1-intro.html` |
| 6.770 | **He opens his laptop**, then the app zooms out of it | `compositions/s2-laptop.html` |
| 12.725 | 24/7 agents: Ava briefs Sam (email, Slack, WhatsApp, grocery list) | `compositions/s3-agents.html` |
| 29.500 | "Now let's get to work, shall we?" | `compositions/title-card.html` |
| 33.100 | Code mode: Issues → #142 → prompt → Opus → send → subagents fix it | `compositions/s5-code.html` |
| 51.900 | "Wanna destroy some bugs with your buddies?" | `compositions/title-card.html` |
| 55.930 | Multiplayer: invite Leo, he joins, both prompt Vivianne | `compositions/s7-multiplayer.html` |

- `scenes.json` holds the scene order and durations. `npm run build` turns it into `index.html`.
- Cut points are timed to the music (`assets/audio/theme-cut.mp3`). The beat
  drops back at 41.9s, exactly when Sam hits send in code mode. Don't change the
  durations without re-checking that.
- The cut deliberately has no end card. More scenes may be added after scene 7.

## The task you're picking up: add the city b-roll

The user has the clip locally (they'll give you the path). It's
`16995919_4320_6480_30fps.mp4`: **4320×6480, portrait**. The trailer is
landscape 1920×1080, so convert it before using it.

1. Convert it: scale to 1920 wide, crop a 1080-tall band from the middle,
   drop the audio, and keep at least 7s (scene 1 is 6.77s; a shorter clip
   leaves black under the title).

   ```bash
   ffmpeg -y -i "<path to 16995919_4320_6480_30fps.mp4>" -t 7 -an \
     -vf "scale=1920:-2,crop=1920:1080:0:(ih-1080)/2,fps=30" \
     -c:v libx264 -pix_fmt yuv420p -crf 18 -preset slow \
     packages/trailer-hyperframes/assets/footage/city.mp4
   ```

   The centre crop keeps about the middle third of the shot's height. Look at
   the result first. If the busy part of the city sits higher or lower, change
   the crop's y offset (`(ih-1080)/2`; for example `(ih-1080)*0.35` is higher,
   `*0.65` is lower) and ask the user which band they want. If the clip is
   shorter than 7s, say so; don't loop it silently.
2. `cd packages/trailer-hyperframes && npm run build`. This adds a muted
   `<video>` under scene 1, and the scene hides its placeholder slate itself.
   No code changes are needed.
3. Check it with `npx hyperframes@0.9.2 snapshot --at 1,3.5,6.5 --no-end` and
   look at the frames. The title should read clearly over the footage. The
   scene already darkens the edges (`.shade` in `s1-intro.html`). If the city
   is too bright behind the white title, strengthen that gradient rather than
   dimming the clip.
4. Render with `npm run render`. It writes `out/vivianne-trailer.mp4` (git-ignored).
   Show the user the video.
5. Commit `assets/footage/city.mp4` (a 1080p 7s clip is a few MB; never commit
   the 8K original), plus any `.shade` tweak. Push to the same branch.

The laptop shot works the same way, via `assets/footage/laptop.mp4` (scene 2,
5.955s). The user hasn't supplied it yet.

## Running it locally

- You need Node ≥ 22, FFmpeg on PATH, and Chrome or Chromium for HyperFrames.
  Run `npx hyperframes@0.9.2 doctor` to check.
- **Windows:** the `test` script quotes a glob as `'scripts/*.test.mjs'`, which
  `cmd`/PowerShell won't expand. Run `node --test scripts/build.test.mjs
  scripts/motion.test.mjs scripts/scope.test.mjs scripts/tokens.test.mjs`
  instead, or use Git Bash.
- Run `npm run check` (HyperFrames lint, layout, motion, contrast). It should
  report **0 errors**. Contrast warnings on the captured app UI are the app's
  own styling; leave them.

## How the app UI scenes work (3, 5, 7)

They show the **real** Vivianne GUI (`packages/gui-next`) running on a fake
Tauri backend with demo data, captured as markup and CSS into `captures/*.js`
and then animated. They are not hand-made mockups.

- Re-capture only if the app UI or the demo copy (`assets/story.js`) changes.
  Run `npm install` at the repo root, start `npx vite --port 5199` in
  `packages/gui-next`, then `npm run capture -- <agents|code|multiplayer|chrome>`,
  then `npm run build`.
- The capture tool lives in `tools/capture/`:
  - `mock-ipc.js` is the fake backend.
  - `scenarios/*.cjs` script each scene's states.
  - `scope.cjs` confines the app CSS to `.viv-app`.
- The capture records element rects with real fonts (`data-cap-rect`), and the
  cursor and camera aim from them through `HF.at` / `HF.focus` in `assets/motion.js`.
- The camera pushes in on every click and on the composer while typing, then
  pulls back after each send.

## Open items the user hasn't decided

- **Integration:** merge to `main` locally, open a PR, or keep the branch. Ask;
  don't assume.
- **Sign-off before the video goes public.** It shows two things the app doesn't
  have yet:
  - an **Issues tab** (added at capture time by `tools/capture/scenarios/issues.js`)
  - a **WhatsApp** integration (it's only Ava's step and message)
- **Two real app bugs** were worked around in the captures only and are still in
  `gui-next`:
  - `ModelSelector.tsx`: `STICKY_HEADER_H = 84`, but the search block is 69px,
    so provider headers cover each group's first model.
  - The floating plan pill (TodoPanel) covers the first user message in a short chat.
- **Known minor issues left for later:**
  - scene roots hard-code their old durations
  - `build.mjs` doesn't frame-align start times, validate input, or escape every attribute
  - captures carry about 5MB of duplicated CSS and reference absent fonts
  - `@layer`/`@supports`/`@container` rules are dropped in capture
  - a little dead code (`rows` in `s3-agents.html`)

## Conventions

- Commit messages end with the session's `Co-Authored-By` trailer, if your
  harness gives one.
- Keep copy in `assets/story.js`, scene order in `scenes.json`, and
  colours/fonts from the app's theme (`npm run tokens`).
- Never use `Math.random`, `Date.now` or network fetches in compositions;
  renders must be deterministic. Use `HF.rng(seed)`.
