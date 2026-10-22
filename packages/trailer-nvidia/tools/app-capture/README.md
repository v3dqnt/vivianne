# App capture

Renders the real gui-next app (light theme) with seeded store state and turns
each state into a layer of `compositions/app.html`. Nothing in the app layer
is hand-drawn: it is the app's own React/Emotion output.

1. Run the gui-next dev server (`npm run dev --workspace=gui-next`, port 1420).
2. Capture each state (writes `out/app-<x>.{png,html,css}`):

       PPTR=<path to puppeteer-core> CHROME=<path to chrome-headless-shell> \
         node capture.cjs seeds/seed-a.js app-a        # …through seed-f

   `seeds/common.js` holds the shared project, threads, plan and helpers;
   each `seed-<x>.js` sets one moment of the story.
3. Build: `python build_app.py <trailer-day> <gui-next> a,b,c,d,e,f`
   → `assets/app/app.css`, `assets/app/fonts/`, `compositions/app.html`
   (from `app.tpl.html`; edit the template, not the output).

Mods: a seed can set `window.__MODFILES` (`'<key>/<path>'` → text, e.g. from the
app's own presets); capture.cjs serves those, and the SDK, at
`http://vivmod.localhost/`, and photographs each mod's `<iframe>` into the dump
(its DOM is out of reach). The browser reports a Windows user agent, as the app
does on Windows. Frame 8 (`seeds/mods-common.js`, b1–b4) builds into its own
`assets/app/app-f08.css` (`CSS=app-f08.css`, see build.sh).

