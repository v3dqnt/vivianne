---
format: 1920x1080
duration: 60s
message: "Every agent you run, in one harness, each task sent to the right Nemotron"
arc: Promise → Engine (Nemotron on Nebius) → Router → How you work → What it remembers → Where it acts → How you reach it → What it builds → Beyond the window → Logo
audience: Nebius x NVIDIA hackathon judges; submission page and socials
mode: collaborative
version: v3
---

## Decisions

- **Message:** Every agent you run, in one harness, each task sent to the right Nemotron.
- **Format:** 1920x1080, 60s, no voiceover, no music bed (sound design only, React Bits Sound Lab). No captions, so no caption keep-out; key copy stays inside title-safe (10% inset).
- **Spine:** one floating Vivianne window in a dark 3D space. Every feature is a layer that slides out of it or that the camera flies into, and at the close every layer flies back into it. The camera never leaves this one world; there are no hard cuts between unrelated cards.
- **Hero prop:** the window (beats 1, 3, 8, 10). Callback: beat 10 answers beat 1 (the same window, now full).
- **Brand:** the white brand system (`DESIGN.md`): canvas `#ffffff`, ink `#111111`, body `#6b6b6b`, mute `rgba(17,17,17,0.4)`, hairline `#e8e8e8`, soft surface `#f6f6f6`, film grain 0.03. The app is captured in its light theme. Accent NVIDIA green `#76b900` (text `#4a7400`), only where Nemotron or the router is on screen. The notch stays hardware-black, as in the app. Logo ring gradient `#4f8fe0 → #b7c2cc → #e9edf0` on the logo only (`DESIGN.md`). Type: display Sora 400/500, tracking -0.03em (brand); app UI is the app's own captured render (Geist); code Geist Mono. Radii: window 14px, cards 10px, pills 999px (app). Easing: `expo.out` for arrivals, `power3.inOut` for camera.
- **Bans:** no fake numbers (costs, speeds); no generic harness features (stop, retry, approve); no glow or neon; no third-party logos besides NVIDIA and Nebius credits (the browser is a generic frame, no Chrome mark); no slideshow (every beat a fresh card) and no screensaver (motion that says nothing).
- **Held frame:** beat 3, the moment the hard task lands in the Ultra lane: everything stops for 1s on "Hard problems go to Ultra."
- **Direction rule:** the camera's travel is forward (into the screen) and left-to-right; returns happen only at the close.
- **Truthfulness:** every app screen is the real gui-next app, captured with seeded state (the `trailer-src/tools/app-capture` pipeline); only the router lanes, memory constellation and terminal are designed. Every claim is in the build (BRIEF.md, Notes).

## Changes from v1

- v3, "can you do it like in the reference i mentioned with proper ui animations??": every app beat now shows the real interface performing (typing, a cursor pressing real buttons, cards building piece by piece, panels opening), like the earlier "A day" trailer and Apple/OpenAI launch films. See each frame's `ui_motion`.

- "white" (user, after studying the earlier trailer): the film moves from the dark NVIDIA theme to the white brand system; the app is captured in light mode with the earlier trailer's capture pipeline instead of being redrawn.

## Still open

- Sound Lab licence (React Bits repo) to confirm before the sounds go in.

## Frame 1 — Open (0–4s, ~4s)

- scene: A cursor blinks on black; the Vivianne logo ring draws itself, the wordmark rises from a mask; "One harness. Every agent." lands under it.
- duration: 4s
- transition_in: cut
- status: built
- voiceover: onscreen
- audio: soft tap on the first blink (0.3s), riser 0.8–2.2s, impact on the wordmark (2.2s)
- constraint: no particle burst, no lens flare
- ui_motion: none (designed title).

**Motion:** slow push-in on z (scale 0.96 → 1) the whole beat; ring stroke draws 0.4–1.6s (expo.out); wordmark rises 2.2s; tagline at 2.9s. **Seam out:** the camera pulls back and the logo shrinks into the title bar of a window (dolly back on z). **Why:** the promise; introduces the window.

## Frame 2 — Engine (4–10s, ~6s)

- scene: The camera dives through the window's chat into a stream of tokens flowing toward the camera; two lines: "Thinks on NVIDIA Nemotron." then "Runs on Nebius Token Factory."
- duration: 6s
- transition_in: dolly-through
- status: built
- voiceover: onscreen
- audio: whoosh on the dive (4.0s), low hum under the stream, soft tap per line
- constraint: no logos larger than the type; NVIDIA and Nebius as words with small marks
- ui_motion: the real chat window (light theme). A cursor opens the real model picker and picks "Auto (cost-aware router)"; the prompt "Plan the database migration for orders" types into the real composer at ~30 chars/s; Send presses down and springs back. Then the camera dives into the composer.

**Motion:** z-dolly through the chat surface (4.0–4.8s); token glyphs stream past on z with parallax; line one at 5.0s, line two at 7.2s, green on "Nemotron". **Seam out:** the stream bends left and becomes a conveyor of task cards (camera pans right on x). **Why:** what powers it, for the sponsors' judges.

## Frame 3 — Router (10–19s, ~9s)

- scene: Three lanes recede in depth, labelled "Nemotron Nano · fastest", "Nemotron Super", "Nemotron Ultra · most capable". Task cards ("Rename these files", "Summarize this thread", "Plan the database migration") pass a grader ("Graded locally") and drop into their lanes. Hold: "Hard problems go to Ultra."
- duration: 9s
- transition_in: whip-pan
- status: built
- voiceover: onscreen
- audio: soft tap per card graded, riser 13.5–15.8s, impact when the hard card lands (15.8s), 1s of silence on the hold
- constraint: no cost figures, no fake latency numbers
- ui_motion: the real Settings → Cost-aware routing panel: the Easy, Medium and Hard rows fill in one by one with Nemotron Nano, Super and Ultra, the Auto switch toggles on. Then the designed lanes take over in depth.

**Motion:** camera on a crane: high three-quarter view, tracking each card along x; the third card is followed down into the Ultra lane (crane down on y); hold 16.0–17.0s; then "Cost-aware routing. Each task, the right model." at 17.2s. **Seam out:** the lanes rotate 90° on y and become one face of a prism. **Why:** the router is the headline technical idea and uses three NVIDIA models.

## Frame 4 — Three ways to work (19–25s, ~6s)

- scene: A three-faced prism turns: "Threads" (a sidebar of conversations), "Agent teams" (a room with teammates), "A whole company" (an org chart under a lead).
- duration: 6s
- transition_in: rotate-y
- status: built
- voiceover: onscreen
- audio: soft whoosh per face turn (19.0, 21.0, 23.0s)
- constraint: no stock avatars: the app's own robot heads
- ui_motion: real captures of the Threads sidebar, an agent team room and the company org chart; on each face rows rise in 0.08s apart and the teammates' robot heads change state (thinking, working).

**Motion:** prism rotates 120° on y per face (power3.inOut, 0.8s), each face the rebuilt UI from `sessions/Sidebar`, `team/TeamRoom`, `team/CompanyTree`. **Seam out:** the camera flies into the org chart's lead node, which opens into a point cloud (z-dolly). **Why:** project modes are specific to Vivianne.

## Frame 5 — Memory (25–31s, ~6s)

- scene: A constellation of memory points; a spec sheet card slides in: "Memory that outlives the chat." Rows: "Embeddings · NVIDIA Nemotron", "Search · meaning + keywords", "Upkeep · tidies itself while you're away".
- duration: 6s
- transition_in: dolly-through
- status: built
- voiceover: onscreen
- audio: shimmer as points connect, soft tap per spec row
- constraint: no brain icon
- ui_motion: the real Memory panel: entries appear one by one and one expands; the designed constellation and spec card sit behind and beside it.

**Motion:** camera drifts through the cloud on z with slow orbit; lines connect related points; spec rows stagger in (0.25s apart). **Seam out:** the points collapse into a browser tab bar (scale down on z, pan up on y). **Why:** long-term memory with a Nemotron embedder.

## Frame 6 — Acts in your world (31–38s, ~7s)

- scene: "Your browser. Signed in as you." a browser frame with the agent clicking through a page; "Any app on your desktop." a desktop window with a highlight box; "Uses your passwords. Never sees them." a password field fills as dots from the vault.
- duration: 7s
- transition_in: scale-down
- status: built
- voiceover: onscreen
- audio: soft taps on clicks, a toggle on the vault fill
- constraint: no real site's logo or brand in the browser frame
- ui_motion: the real approval prompt ("Fill your github.com password?") appears, the cursor presses Allow, and the browser field fills with dots; the real Vault list in Settings shows the entry with its value hidden.

**Motion:** three panes in a row at different depths; whip pans along x between them (0.5s each, motion blur); the vault field fills character by character as dots. **Seam out:** pan up on y to the top edge of a screen. **Why:** real-world action with guardrails, specific to Vivianne.

## Frame 7 — Viv Instant (38–44s, ~6s)

- scene: The notch drops from the top edge; the robot face talks; words appear as they are spoken: "Three agents are working. On Vivianne, the lead is fixing the build." Title: "Hold a key. Just say it."
- duration: 6s
- transition_in: tilt-up
- status: built
- voiceover: onscreen
- audio: notification as the notch opens, soft ticks under the words
- constraint: no microphone icon
- ui_motion: the real notch overlay: it grows from the top edge, the robot face switches to speaking, and the answer reveals word by word (the app's own word-by-word component).

**Motion:** the notch grows from the top edge (width, then height, eased), face animates, words reveal one by one (the app's own `RevealedText`). **Seam out:** the notch's text card detaches and flies forward to become a summary card (z toward camera). **Why:** voice access to every agent and project.

## Frame 8 — It builds its own interface (64.2–72.6s in the cut, ~8.4s)

- scene: Asked for "a panel that shows every agent at work", Vivianne reads the mod guide and writes a mod (`mod.json`, `panel.html`). In Settings → Mods the new "Agent board" waits, off; its switch brings up the consent dialog ("Turn on Agent board?", one permission: see what your agents are doing); Turn on. The board slides into the dock with a live card per agent, and a later answer's ```` ```checklist ```` is drawn by the Checklist mod in the chat. Titles: "Ask for a panel. It builds the mod." → "Nothing runs until you turn it on." → "Make it yours." with "Panels, sidebar sections, chat components, tools and themes. Sandboxed."
- duration: 8.4s
- transition_in: blur-in (as the scenes before it)
- status: built (`compositions/f08-mods.html`, states b1, b2, b4, b3)
- voiceover: onscreen
- audio: whoosh in, soft taps per step, pop as Settings opens, click on the switch, notification on the dialog, click on Turn on, whoosh as the board arrives, sparkle under the cards, pop on the checklist, low impact under "Make it yours."
- constraint: only what 1.8.2 ships: the mods are the app's own presets (Agent dashboard, Checklist), run sandboxed in the capture and photographed; permission words from `mods::manifest`.
- ui_motion: the chat builds (ask, the three steps, the reply); Settings → Mods opens; the cursor presses the Agent board's switch, the real consent dialog pops, the cursor presses Turn on; the board's cards fly into the dock; the second exchange rises and the checklist pops in.

**Motion:** arrives from below-left on a tilt, pushes into the chat, leans to the switch and the dialog, pulls back as the board lands, pushes into the dock, pans to the checklist, settles wide for the title. **Changed from v3:** the earlier "summary card", "Show panels stacked" and "Quick access" beat isn't in the build; mods are. Sidebar sections are left out: in a threads project the app doesn't show mods' sidebar sections (`Sidebar.tsx` renders `SidebarMods` only for team and company projects). **Why:** mods are specific to Vivianne, and she can build them herself.

## Frame 9 — Beyond the window (50–55s, ~5s)

- scene: A terminal: `$ viv run "fix the failing test" --model auto` then `step 1: read`, `step 2: edit`, `done`; then `$ viv load qwen3-8b` and "Loaded · http://127.0.0.1:8080/v1". Title: "Out of the window, too."
- duration: 5s
- transition_in: tilt-x
- status: built
- voiceover: onscreen
- audio: soft key taps while typing, a tick per step line
- constraint: output lines taken from real `viv` output
- ui_motion: none (designed terminal; output text copied from real `viv` runs).

**Motion:** terminal in 3D tilt, typing at a steady rate, lines appear; camera pushes in slightly. **Seam out:** everything flies back toward the window (reverse z), the only return move in the film. **Why:** the CLI and local models make it a harness, not only an app.

## Frame 10 — Close (55–60s, ~5s)

- scene: Every layer flies back into the window, the window folds into the logo; "Vivianne" and "Built on NVIDIA Nemotron · Nebius Token Factory".
- duration: 5s
- transition_in: converge
- status: built
- voiceover: onscreen
- audio: reverse whoosh as layers return (55.0s), impact on the logo (56.6s), tail out
- constraint: no static end card: the logo lands while layers are still settling
- ui_motion: none (designed close).

**Motion:** layers converge on z (power3.inOut 1.4s), window scales into the logo ring (callback to beat 1), credit line at 57.4s, gentle drift to the end. **Why:** close on the name and the sponsors.
