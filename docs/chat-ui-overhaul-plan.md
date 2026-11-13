# Chat UI overhaul: a quieter transcript

Goal: Vivianne's chat reads like the reference (GitHub Copilot's chat, three screenshots from the user, 2026-11-13): the conversation is plain text, and everything the agent does is a thin, quiet timeline you can open, not a stack of boxes.

## What the reference does

1. **Activity is a timeline of one-line steps.** Each step is an icon, a verb phrase and a chevron: "View file PRODUCT.md", "Get data from /repos/…/contents". Steps are grouped under a heading such as "Using results of get file and 2 other tool calls", and the group opens to the same one-line steps. A step opens to its result (a small file chip, or the log).
2. **No chrome around activity.** No borders, no card headers, no coloured banners. Muted text, one size, one rhythm.
3. **The agent has a face.** A small avatar sits to the left of the first activity line of a turn. The reply text is flush with no bubble, with inline code chips and links.
4. **The reply ends with a quiet action row**: copy, retry, thumbs, "more". Small icons, not text buttons.
5. **Your message is a right-aligned pill.** A context chip (the repo) sits top right.
6. **The composer is one rounded field.** Placeholder "Ask anything or type @ to add context". Bottom row: icon buttons on the left; on the right a model chip ("Auto"), one preset ("Optimized for: Balance") and send.
7. **Failures are plain sentences.** Cause first ("The job failed during Install dependencies…"), the evidence after it, no alert box.

## Where Vivianne is now (baseline: the gallery's "Main chat" scene)

Already close: one reading column (`CHAT_COLUMN` 900), a right-aligned user pill, borderless one-line tool groups ("Ran 2 commands · 1 failed"), plain prose, a one-field composer.

Gaps:

| Reference | Vivianne today |
|---|---|
| Open group shows one-line steps with icons | Open group shows `ToolCallCard`s: bordered cards with headers (`ToolCallCard.tsx`, 576 lines) |
| Collapsed group names the work ("Using results of …") | Collapsed group is a summary string; the live call replaces it while running |
| Avatar on the agent's activity | None |
| Quiet action row after the reply | Copy/speak icons exist in `MessageBubble.tsx`; no retry, no row design, not tied to turn end |
| Failures inline | A boxed "Stopped here" card; checkpoint cards; handoff dividers; permission dialogs as modals |
| Composer: + / @ left, model chip + preset + send right | Approval mode, thinking level, model, mic, queue and context all in the toolbar (`ChatComposer.tsx` 546 lines, `Composer.tsx` 476) |

## Plan

Each phase ships on its own and is checked in the gallery (dark and light) before the next starts.

**P0. Baseline scenes.** Add gallery scenes with fixtures that exercise the hard cases: a long run (12 steps, mixed tools), a failed step, a live run, an approval waiting, a stopped turn, an error, and a narrow window. Screenshot each as the "before". Add the reference as a design note in the scene.

**P1. Tokens.** In `lib/chatStyle.ts`: one type scale for activity (13px steps, 12px meta), muted colours for activity text, spacing in multiples of 4, and no border or fill on activity rows or wells (`TOOL_ROW_SX`, `TOOL_BODY_SX`, `TOOL_WELL_SX`). No behaviour change.

**P2. The timeline.** The core of the overhaul.
- New `ActivityRow` (icon via the existing `iconFor`, verb phrase via `declarationFor`, target, chevron, spinner while running).
- `ToolGroup` opens to a list of `ActivityRow`s, each opening to its result. Heading: "Using results of read and 2 other calls" style, built from `summarizeTools`; the running step still takes over the heading.
- `ToolCallCard` loses its card chrome and becomes the body shown inside an open row (diff, code, shell outcome, images). File results become `FileLink` chips.
- A lone call (the `groupTranscript` rule keeps one call as its own item) uses the same `ActivityRow`, so single and grouped calls look alike.
- Rows mount only while open (as `ToolGroup` does now) so a long run stays cheap.

**P3. The agent's face and the reply footer.**
- Avatar mark beside the first activity line of each assistant turn (reuse `AgentMark`).
- The action row under the last message of a turn: copy, retry (regenerate from this turn), speak, branch. Visible on hover and on the latest turn; the timing/token detail moves into one info popover.

**P4. Inline status.**
- "Stopped here", errors, checkpoints and handoffs become one muted inline sentence with an inline action ("Retry", "Resume"), no box.
- Permission and question requests render as inline cards in the transcript (the same approval widgets the 24/7 chat uses), not as modals. Stop stays one click.

**P5. Composer.**
- Left: `+` (attach), `@` (context). Right: model chip, one preset chip, send.
- Approval mode, thinking level and mic move into the model chip's popover. The queue, drafts, slash menu and mention menu keep working as they do.
- Placeholder: "Ask anything or type @ to add context".

**P6. Thread header.** The project and branch chip (`ProjectPill`) pinned top right of the transcript; remove it from the places it now repeats.

**P7. Polish.** Light and dark, narrow widths, density and chat-width settings (Settings → Appearance), reduced motion, keyboard (rows are `role=button` with `aria-expanded` already), scroll anchoring while rows open and close.

## Risks

- **Scroll anchoring.** Row heights change when a row opens during streaming; the list must not jump. Check `MessageList.tsx` before P2.
- **Streaming cost.** A long run now has many more rows; keep result bodies unmounted until opened, and measure a 200-step run.
- **Grouping tests.** `groupTranscript` has tests (`toolGroups`); the lone-call rule changes meaning when single calls also use rows, so update them with the change.
- **Hidden state.** Per-step expansion state must survive re-renders and the group re-grouping as new messages arrive.
- **Settings.** `--chat-max-width`, `--chat-font-*` and `--chat-header-height` must keep working.

## Decisions to confirm (recommendations first)

1. Steps default **collapsed**, the running step open. (Alternative: open the failed step automatically; recommended too.)
2. Keep the group threshold at two calls; a single call gets the same row, not a heading.
3. Per-tool icons are monochrome Solar glyphs from the existing set; no new icon family.
4. The reply action row shows on hover for older turns and always for the latest.

## Out of scope

The sidebar, rail, home screen, and the 24/7 agents' chat (which already uses the inline approval widgets this plan reuses).

## Done when

The gallery's "Main chat" scene and the new scenes match the reference's density and calm in both themes; a 200-step run scrolls smoothly; `tsc` and the web tests pass; and the real app has been run through a tool-heavy conversation.
