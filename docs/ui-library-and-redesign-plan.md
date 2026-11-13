# Vivianne's own UI library on a HeroUI fork, and the chat redesign

Two goals that fit together: (1) replace the hand-rolled `ui/` kit (Radix + Emotion) with **Vivianne's own component library built on the fork** `v3dqnt/heroui-fork-for-viv`, and (2) use it to redesign the app, starting with the chat, to the calm look of GitHub Copilot's chat (the user's two screenshots, 2026-11-13).

## What the fork is

`heroui-fork-for-viv` is HeroUI **v3.2.7**, currently identical to upstream (its last commit is upstream's). A pnpm + Turborepo monorepo:

- `packages/react` (`@heroui/react`): ~80 components on **React Aria Components**: accordion, alert, alert-dialog, autocomplete, avatar, badge, breadcrumbs, button, calendar, card, checkbox, chip, color pickers, combo-box, date pickers, disclosure, drawer, dropdown, empty-state, input, input-otp, kbd, link, list-box, menu, meter, modal, number-field, pagination, popover, progress, radio, scroll-shadow, search-field, select, separator, sheet, skeleton, slider, spinner, surface, switch, table, tabs, tag-group, textarea, toast, toggle-button, toolbar, tooltip.
- `packages/styles` (`@heroui/styles`): the CSS. One `.css` per component, themes as CSS variables (`themes/`), variants as `tailwind-variants` (`src/components/*.styles.ts`).
- Tailwind CSS **v4**, React **19**, Node 22+, Apache-2.0.

Not in HeroUI: layout primitives (`Box`, `Stack`) and a `sx` prop. Everything is `className` + Tailwind.

## Where Vivianne is

- Already on **React 19.2** and **Tailwind 4.3** (the same majors HeroUI v3 needs), so the toolchains agree. Tailwind is imported as utilities only, with no preflight.
- The kit in `src/ui` is MUI-shaped (Emotion, an `sx` prop): **2,703 `sx={...}` uses in 178 files**; 176 files import the kit.
- Usage by weight: `Typography` 143 files, `Box` 141, `Stack` 133, `ButtonBase` 74, `Button` 63, `useTheme` 49, `alpha` 41, `CircularProgress` 38, `IconButton` 36, `Tooltip` 34, `InputBase` 33, `Dialog` 30 (+ title, content, actions), `Alert` 25, `TextField` 19, `Divider` 18, `Switch` 13, `Collapse` 13, `MenuItem` 11, toggle buttons 9 each, then a long tail.

So the hard part is not the components: it is `sx` and the layout primitives, which HeroUI does not have and the app uses everywhere.

## Architecture

**A new package, `@vivianne/ui`** (working name), separate from the app, built on the fork:

```
heroui-fork-for-viv (the fork: upstream stays mergeable)
 ├─ packages/react, packages/styles      untouched except for fixes we send upstream
 └─ packages/viv-ui   @vivianne/ui       ← new
      ├─ theme/viv.css      Vivianne's tokens (colour, radius, type scale, density) as HeroUI CSS variables
      ├─ components/        Vivianne components: re-exports of HeroUI parts, restyled, plus the ones HeroUI lacks
      └─ primitives/        Box, Stack, Text with a className API (no sx)
Vivianne app (packages/frontend) imports only from @vivianne/ui
```

- **The theme is the product.** The Terax look and the Copilot-like calm are expressed as CSS variables in `viv.css`, so every component follows it and light/dark/accent palettes (today's `palettes.ts`) become theme files.
- **Keep the fork clean.** Upstream stays mergeable: our theme and our own components live in `viv-ui`, not inside `packages/react`. Bug fixes we want everywhere go upstream.
- **Components HeroUI does not have** and the app needs (build in `viv-ui` on React Aria): `Collapse`, `Orb`, `ToggleButtonGroup` variants, the sliding `SegmentedTabs`, `CircularProgress` styling, a command/slash menu, a resizable panel handle, a chat composer shell.
- **Licence:** HeroUI is Apache-2.0: keep its LICENSE and NOTICE in the package.

## How the app moves over (strangler, never a big bang)

1. **Same names, new insides.** Reimplement the app's `ui/` exports on top of `@vivianne/ui` with the same names and props where they map (`Button`, `IconButton`, `Switch`, `Tooltip`, `Dialog*`, `Select`, `Menu*`, `Popover`, `Alert`, `Chip`, `Slider`, `TextField`, `InputBase`, `Divider`, `Link`). Then 176 files keep compiling and the look changes everywhere at once. Today's `ButtonBase` maps to HeroUI's button parts; `contained/outlined/text` map to HeroUI variants.
2. **Layout primitives stay.** `Box`, `Stack`, `Typography` and `sx` remain (Emotion) as the compatibility layer, so nothing in the 2,703 `sx` uses has to change on day one.
3. **Codemod to classes, module by module.** A script rewrites `sx` to `className` Tailwind utilities for the common shapes (`display`, `flex`, `gap`, `p`, `m`, `width`, colours, radius). The residue is hand-fixed. Order: chat first, then settings, rail/team, the rest.
4. **Retire Emotion** when `sx` count reaches zero; delete the compatibility layer and `src/ui`.

At each step the gallery (`npm run gallery`) is the check: every component and screen in light and dark.

## The chat redesign (the first thing the new library is used for)

What the Copilot screenshots show, mapped to Vivianne:

| In the reference | In Vivianne |
|---|---|
| A collapsed status line "Thought for 12s" with a spinner glyph | Reasoning becomes one muted line: "Thought for 12s", opens to the text |
| System lines with a mark: "Model changed to Copilot Fusion", "Auto selected X because it was the best choice out of the models available in your plan" | Notices for model changes and **auto-routing decisions** (Vivianne already routes models) as one muted line with the app mark, replacing boxed banners |
| Tool activity as a line: "> Read C:\...README.md and C:\..." | The planned timeline row (see `chat-ui-overhaul-plan.md`): icon, verb, target, chevron, opens to the result |
| Plain prose and bullet lists, no bubble, generous line height | Already close; tune type scale and spacing in the theme |
| Under each reply a small action row: bookmark, copy, info, retry, more, then **the model that wrote it** ("Copilot Fusion · mai-code-1.1-flash") | Same row: save, copy, info (tokens, time, cost), retry, more, with the model name beside it. The model attribution is new and useful with routing |
| The composer: one rounded field, placeholder "Ask anything. Use / for commands, @ files, & sessions, # issues…" | Same field and placeholder. Today's `/` and `@` stay; add `&` (jump to or mention another thread or session) and `#` (issues and pull requests) |
| Bottom row: `+`, a mode chip ("Interactive"), a model chip ("Copilot Fusion · Standard"), then a busy spinner, mic, send | `+`, mode chip (our approval/plan modes), model chip, then spinner, mic, send. Thinking level moves into the model chip's popover |

This replaces phases P1 to P6 of `chat-ui-overhaul-plan.md`: the same targets, built from the new library instead of the old kit.

## Phases

**L0. Decisions and spike (about a day).** Make `packages/viv-ui` in the fork, install it into the app via a local link, and render one HeroUI `Button`, `Tooltip` and `Dialog` inside the real app with a first `viv.css` theme. Proves the two stacks (Emotion app, Tailwind v4 library) coexist, and settles the reset question below.

**L1. Theme.** Port the tokens (`palettes.ts`, `ui/scale.ts`, fonts) to `viv.css`: dark, light, accents, density. The chat's measurements (column 900, 12px radius, 8px gutter) become variables.

**L2. Component parity.** Rebuild the 15 most-used kit parts on HeroUI under the old names; add the missing ones (`Collapse`, `Orb`, segmented tabs, command menu). Gallery shows each in its states. Swap the app's `ui/index.ts` to export them. App compiles, look changes.

**L3. Chat.** Build the new chat from the library: reasoning line, notice lines, timeline rows, reply action row with model attribution, composer with `/ @ & #`.

**L4. The rest, module by module,** with the `sx` codemod: sessions and rail, then team and git, then settings.

**L5. Retire.** Remove Emotion, the compatibility layer and `src/ui`.

## Risks and open decisions

1. **How the app consumes the library.** npm cannot install a sub-folder of a git repo, and the fork uses pnpm while the app uses npm. Options: (a) develop with a local link and publish tarballs/GitHub Packages for CI; (b) vendor the built `viv-ui` into the app repo as `packages/ui` and sync from the fork; (c) move the whole app to pnpm. Recommendation: **(a) a private GitHub Packages publish of `@vivianne/ui`**, with `npm link` for local work.
2. **CSS reset.** HeroUI's styles assume Tailwind's base; the app deliberately has no preflight and its own `CssBaseline`. Decide in L0: adopt HeroUI/Tailwind base and delete the app's reset (cleaner), or scope HeroUI to a layer.
3. **Bundle size and startup** (a Tauri webview; React Aria adds weight). Measure in L0 and L2, import per component.
4. **Two systems during the move.** For a while the app has Emotion and Tailwind side by side; keep the compatibility layer thin and delete it on schedule.
5. **Mods and widgets** (`mods/sdk`, `WidgetBlock`) use `--viv-*` CSS variables; keep those names stable in `viv.css`.
6. **Windows-only overlays** (notch, screen-control overlay) are separate windows with their own bundle entry: give them the theme too.

## Decisions to confirm (recommendations first)

1. Library lives in the fork as `packages/viv-ui`, published as `@vivianne/ui`.
2. Strangler migration with a thin `sx` compatibility layer; codemod after parity.
3. Adopt Tailwind's base reset and drop the app's own (decide for certain after the L0 spike).
4. Start with the L0 spike now.
