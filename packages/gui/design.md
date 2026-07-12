# Vivianne GUI — Design System

This document is the source of truth for the visual language of the Vivianne desktop app. Read it before adding or restyling any component. The goal is one consistent system, not a new pattern per screen.

## Philosophy

Vivianne is a coding-agent harness, not a chat toy. The UI should read like a clean IDE/agent log, not a messaging app:

- **Activity feed, not chat bubbles.** Assistant turns, reasoning, and tool calls render as a vertical stack of rows — not rounded speech bubbles. Only the user's own message gets a distinct (but still subtle) treatment, since it's the one thing the user needs to visually anchor on while scanning a long turn.
- **Collapsed by default, dense when expanded.** Tool calls (read/write/edit/bash/grep/find), reasoning blocks, and diffs are collapsible rows: icon, label, one-line summary, right-aligned meta, chevron. Expand for the full body.
- **Warm, quiet, monochrome.** One accent (blue, focus/interactive only). Everything else is charcoal-on-cream at varying opacities. No colored button fills, no drop shadows beyond a 1px hairline.
- **Numbers and paths are always monospace.** Token counts, file paths, commands, diffs — mono. Prose is sans.

## Color Tokens

Defined in `src/app/globals.css` under `@theme`. Never hardcode a hex in a component — use the Tailwind utility the token generates (`bg-canvas`, `text-body`, `border-hairline`, etc).

| Token | Value | Usage |
|---|---|---|
| `--color-canvas` | `#f7f4ed` (Cream) | Page/app background |
| `--color-ink` | `#1c1c1c` (Charcoal) | Primary text, headings, dark button fill |
| `--color-on-primary` / `--color-on-dark` | `#fcfbf8` (Off-White) | Text/icons placed on a charcoal fill |
| `--color-ink-strong` | `rgba(28,28,28,0.83)` (Charcoal 83%) | Strong secondary text — one step down from full-strength headings |
| `--color-body` | `#5f5f5d` (Muted Gray) | Body copy, captions, secondary labels |
| `--color-hairline-strong` | `rgba(28,28,28,0.4)` (Charcoal 40%) | Interactive/focusable borders (inputs, active toggles) |
| `--color-surface-soft` | `rgba(28,28,28,0.04)` (Charcoal 4%) | Hover tint on rows/buttons |
| `--color-surface-subtle` | `rgba(28,28,28,0.03)` (Charcoal 3%) | Resting fill for recessed surfaces (code blocks, textarea wells) |
| `--color-hairline` | `#ecead4` (Light Cream) | Default borders/dividers |
| `--color-focus-ring` | `rgba(59,130,246,0.5)` (Ring Blue) | Focus ring — the *only* accent color in the system |
| `--color-surface-elevated` | `#fcfbf8` (Off-White) | Dialogs, dropdowns, popovers — anything that should read as "lifted" off the cream page |
| `--color-ink-deep` | `#141414` (derived, not in source palette) | Hover state for solid charcoal buttons — a hair darker than `ink` |
| `--color-mute` | `rgba(28,28,28,0.4)` | Lowest-emphasis text (placeholders, disabled, timestamps) |

Status colors (success/error/running) are the only other hues allowed, and only inside activity rows — never in chrome (sidebar, top bar, composer): emerald for success, rose for error, sky for running, all at ~15% background opacity with a solid-color label.

## Typography

- Display: **Nunito** (`--font-display`) — reserved for the empty-state headline only.
- Sans: **Inter** (`--font-sans`) — everything else: labels, body, buttons.
- Mono: **JetBrains Mono** (`--font-mono`) — paths, commands, diffs, token counts, code blocks.

Sizes in practice: `10px` uppercase tracked labels (section headers, badges) · `11–12px` meta/secondary · `13–14px` body · `18–20px` headings. Don't invent a new size — reuse one of these.

## Spacing & Shape

- 8px base spacing scale (already in `@theme`, tokens `--spacing-*`).
- Radius: pills (`rounded-full`) for buttons, badges, and the composer; `rounded-xl`/`rounded-2xl` (12–16px) for cards, rows, and dialogs. Never a sharp corner on an interactive element.
- Borders are 1px, always `border-hairline` at rest. Don't use shadows for elevation on the base canvas — use the `surface-elevated` token plus a hairline border instead. Dialogs may use a soft shadow since they float above everything.

## Iconography

All icons come from **hugeicons** (`@hugeicons/react` + `@hugeicons/core-free-icons`), rendered via `<HugeiconsIcon icon={...} size={n} />`. Never mix in another icon set.

- Chrome icons (sidebar toolbar, top bar): 16–18px.
- Row/inline icons (tool activity, badges, buttons): 12–14px.
- Icons inherit text color via `currentColor` — set color with a `text-*` class on the icon or its wrapper, never a `primaryColor` prop override, so it stays theme-consistent.

## Motion

Powered by **motion** (`motion/react`, the motion.dev package — not `framer-motion`).

- Standard easing: `[0.16, 1, 0.3, 1]` (stored as `--ease-smooth` in globals.css), duration 150–250ms for entrances, 120–150ms for hover/press feedback.
- Shared-element pills (active tab, active nav item) use `layoutId` + a spring (`bounce: 0.2, duration: 0.4`) — see `components/ui/tabs.tsx` for the reference implementation.
- Activity rows animate in with a small `y: 6 → 0` + fade on mount; collapsible bodies animate `height`/`opacity` on expand, never a hard cut.
- Nothing loops or auto-plays except a genuine in-progress indicator (spinner icon, streaming cursor).

## Component Patterns

### Sidebar
- Top toolbar: new-session icon button, search icon button, filter/settings icon button — icon-only, no labels, 16px icons, ghost hover state (`surface-soft`).
- Session list is grouped by relative date (`Today`, `Yesterday`, weekday name for the last 7 days, then `MMM D`) using each session's `created_at`. Group labels are 10px uppercase `text-mute`.
- Each row: title (truncate) + right-aligned relative time (`text-mute`, 11px). Active row gets `surface-soft` fill, no border. Hover reveals fork/clone icon actions on the right, pushing the timestamp out.

### Top Bar
- Left: sidebar-toggle icon, session name (falls back to "New session" + created time), a small mono usage badge (`context% · N msgs`) once a session has real stats — built from `cmd_agent_session_stats`, never a fabricated number.
- Right: model name + thinking-level, same as before.

### Activity Row (tool calls / reasoning — replaces the old dark "terminal card")
- Light surface (`canvas`/`surface-elevated`), `hairline` border, `rounded-xl`.
- Header: icon (colored per tool, see `ToolCards/toolMeta.ts`) → label → one-line mono summary (truncates) → status badge → chevron, all in a single row.
- Body (collapsed by default only for **Arguments**; **Output** starts expanded to match the reference): monospace, `surface-subtle` background for the actual output text so it still reads as "terminal-like" without going full dark-mode.
- Bash keeps a `$ command` line at the top of its output in a slightly stronger mono tone; edit keeps the Removed/Added diff blocks.

### Composer
- Row 1 (optional, above the input): mode / model / extra-tool dropdown chips, small pill buttons, left-aligned.
- Row 2: textarea (borderless, sits inside the composer well) + stop button (destructive, only visible while generating) + send button (primary pill, `SentIcon`).
- Row 3: meta strip below the composer — left: session/model name; right: context-window percentage. Mono, 10–11px, `text-mute`.
- The whole composer is one `surface-subtle` well with a single hairline border — not nested boxes.

### Settings
- Tab bar uses the shared `ui/tabs.tsx` (animated pill, `layoutId`).
- Every settings section is a `rounded-2xl` `hairline`-bordered card with a bold 14px label + 12px `body` description + the control, right-aligned when it's a single control.
- Destructive actions live in a clearly labeled "Danger Zone" card and always confirm via `ui/dialog.tsx`, never `window.confirm`.

## Do / Don't

- Do reuse `components/ui/*` primitives (button, dialog, tabs, dropdown-menu, input, textarea, select, switch, badge, separator, tooltip, scroll-area). Don't hand-roll a new button or modal.
- Do use `cn()` from `@/lib/utils` for conditional classes. Don't string-concatenate class names.
- Don't introduce a second color for "active" state — active is always `ink`/`on-primary` (solid charcoal fill) or `surface-soft` (light fill), never blue. Blue is focus-ring only.
- Don't add drop shadows to inline chrome (sidebar rows, composer, activity rows). Reserve shadow for dialogs/popovers only.
