---
workflow: general-video
flow: companion
storyboard: yes
message: "Every agent you run, in one harness, each task sent to the right Nemotron"
destination: youtube
aspect: 1920x1080
language: en
audience: Nebius x NVIDIA hackathon judges and viewers of the submission page and socials
length: 60s
angle: launch-trailer
voice: none
---

## Intent

A ~60s launch trailer for Vivianne for the NVIDIA x Nebius Global AI Hackathon (Personal AI track), separate from the 3-minute demo video in `../vivianne-demo-video`. In the user's words: "an apple or anthropic or open ai style motion graphic trailer", "very lively", "the camera moving in all x y and z axes with different kinda transitions, not just slides, the transitions should be smooth".

Show Vivianne as an agentic harness through what is specific to it, not what every harness has ("say like u can stop the agent mid work, those are normal things ... do it properly"). Show that inference is powered by Nebius and NVIDIA Nemotron, and the cost-aware router with three NVIDIA models from fastest to most capable.

## Assets

- assets/fonts/Sora-Variable.ttf — display type (brand, DESIGN.md)
- assets/fonts/Geist-Variable.woff2, GeistMono-Variable.woff2 — the app's own UI and mono fonts, for the recreated UI and terminal (OFL, licence alongside)
- assets/brand/vivianne-logo.svg — the logo, for the open and the close

## Customizations

- No voiceover: kinetic type plus sound design.
- Sound design from React Bits Sound Lab (https://reactbits.dev/tools/sound-lab): whooshes on seams, impacts on title hits, risers into reveals, soft taps on UI, a notification for the notch. "use all kinda sounds from here." Licence to be confirmed from the React Bits repo before use.
- Camera moves in x, y and z; every seam a different smooth transition; no slide-to-slide cuts.
- Vivianne's UI is the real app captured with seeded state (the earlier trailer's `tools/app-capture` pipeline), at the user's direction to recreate it from the app's own components.
- White brand look (user: "white"), as in the earlier "A day" trailer.

## Notes

- Hackathon rules: no copyrighted music; sponsor credits for NVIDIA and Nebius only; say only what is true of the build.
- Router facts from the code: Easy → Nemotron Nano, Medium → Nemotron Super, Hard → Nemotron Ultra, on Nebius Token Factory (`hackathon/edition.ts`, `Quickstart.tsx`); a local difficulty grader (Laya) picks the tier per task.
- Left out because unverified on a real account: renting Nebius GPU jobs.
- No invented numbers (costs, speeds): use relative words ("fastest", "most capable").
