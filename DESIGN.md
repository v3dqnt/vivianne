# Design System: Vivianne Site

> **Purpose:** This file is the single source of truth for the Vivianne marketing site's visual language. Any agent creating, editing, or extending pages or components must follow these guidelines to ensure visual and structural coherence.

---

## 1. Visual Theme & Atmosphere

**Mood:** Quietly confident, monochrome-dominant, engineered precision.
Think of milled aluminium and frosted glass — the interface feels like a piece of hardware, not a poster. It is spacious and unhurried, with generous whitespace and deliberately slow, spring-eased motion. Colour is withheld almost completely; the logo's sky-blue-to-brushed-silver ring is the single accent moment in the entire palette, making it feel earned rather than decorative.

**Density:** Airy. Sections breathe with `py-32 md:py-40` vertical rhythm. Content never exceeds `max-w-7xl` (1280 px) and most prose caps at 46–48 ch for comfortable reading.

**Texture:** A fixed full-screen film grain overlay (`opacity: 0.03`, `mix-blend-mode: overlay`) gives the white canvas a photographic, tactile quality without calling attention to itself.

---

## 2. Color Palette & Roles

The palette is intentionally monochrome. Every colour below is derived from a single ink value (`#111111`) at varying opacities, plus a pure white canvas.

| Token               | Value                          | Role                                                       |
|----------------------|--------------------------------|-------------------------------------------------------------|
| `--canvas`           | `#ffffff`                      | Page background. Pure white.                                |
| `--surface-raised`   | `#ffffff`                      | Card / container surface that sits "above" the canvas.      |
| `--ink`              | `#111111`                      | Primary text colour. Near-black.                            |
| `--ink-strong`       | `rgba(17,17,17,0.82)`          | Secondary emphasis — slightly softened ink.                  |
| `--body`             | `#6b6b6b`                      | Body / paragraph text. Warm mid-grey.                       |
| `--mute`             | `rgba(17,17,17,0.4)`           | De-emphasised labels, metadata, eyebrow text.               |
| `--hairline`         | `#e8e8e8`                      | Default border / divider. Barely visible.                   |
| `--hairline-strong`  | `rgba(17,17,17,0.38)`          | Stronger borders — dashed placeholder outlines.             |
| `--surface-soft`     | `rgba(17,17,17,0.04)`          | Subtle tinted backgrounds — icon circles, eyebrow badges.   |
| `--surface-subtle`   | `rgba(17,17,17,0.03)`          | The lightest tint, used sparingly.                          |
| `--logo-ring`        | `linear-gradient(135deg, #4f8fe0 0%, #b7c2cc 55%, #e9edf0 100%)` | Reserved for the logo. Not used elsewhere. |

### Colour rules

- **Never introduce a new hue.** If something needs emphasis, vary weight or opacity of `--ink`.
- **The logo gradient is sacred.** Do not re-use `#4f8fe0` or the ring gradient on buttons, links, or backgrounds.
- **Selection highlight:** `background: var(--ink); color: var(--canvas)` — inverted monochrome.

---

## 3. Typography

| Property         | Value                                                     |
|------------------|-----------------------------------------------------------|
| **Family**       | Sora (variable, self-hosted). Fallback: `system-ui, sans-serif`. |
| **Loading**      | `next/font/local` with `font-display: swap`.              |
| **CSS variable** | `--font-sora`, exposed via `@theme inline`.               |
| **Antialiasing** | `antialiased` class on `<html>`.                          |

### Type scale & usage

| Element              | Classes                                                                          |
|----------------------|----------------------------------------------------------------------------------|
| **Hero h1**          | `text-4xl md:text-6xl font-medium leading-[1.05] tracking-tight`                |
| **Section h2**       | `text-2xl md:text-3xl font-medium tracking-tight` (or `text-3xl md:text-4xl` for closing CTAs) |
| **Card title**       | `text-base font-medium` (via `<Card.Title>`)                                     |
| **Body paragraph**   | `text-lg leading-relaxed text-body` (hero), `text-sm leading-relaxed text-body` (cards) |
| **Eyebrow**          | `10px / font-weight 500 / uppercase / letter-spacing 0.2em / text-mute` (`.eyebrow` class) |
| **Metadata labels**  | `text-xs font-medium tracking-wide text-mute`                                    |
| **Nav wordmark**     | `text-[15px] font-medium tracking-tight`                                         |

### Typography rules

- **Only use `font-medium` (500) and the implicit normal weight (400).** No bold, no semibold, no light on text. Weight hierarchy is established through size and colour, not heaviness.
- **`tracking-tight` on headings.** Never on body text.
- **`leading-relaxed` on body.** Headings use tight leading (1.05–1.1).
- **Max character widths** are always constrained: `max-w-[18ch]` for hero h1, `max-w-[48ch]` for body, `max-w-[28ch]`–`max-w-[30ch]` for section h2s.

---

## 4. Component Stylings

### 4a. Bezel Cards (`.bezel-shell` + `.bezel-core`)

The signature component — a double-layered card that reads as machined hardware rather than a flat rectangle.

- **Outer tray (`.bezel-shell`):** `border-radius: var(--radius-brand)` (28 px). Pale tinted background (`rgb(17 17 17 / 0.03)`). 1 px inset shadow for subtle rim. Inner padding: `6px`.
- **Inner core (`.bezel-core`):** `border-radius: var(--radius-inner)` (18 px, calculated as `--radius-brand - 10px`). White surface. Layered shadow: a 1 px white inset highlight on top + the `--shadow-float` stack beneath. Transitions on `transform` and `box-shadow` at `400ms var(--ease-spring)`.
- **Hover:** `translateY(-2px)` lift. No colour change, no opacity shift.
- **Content padding:** `p-8` for standard cards, `p-6` for compact/horizontal cards.

### 4b. Buttons (via HeroUI `buttonVariants`)

- **Shape:** Pill (full rounding inherited from HeroUI defaults, matching `--radius-brand`).
- **Colour:** `--ink` background, `--canvas` foreground (via `--accent` / `--accent-foreground`).
- **Sizes used:** `lg` for primary CTAs, `sm` for nav.
- **Trailing icon pattern (`.btn-icon-circle`):** A circular chip (`w-1.75rem h-1.75rem rounded-full bg-white/16`) nested inside the pill. On `.group:hover`, translates `(2px, -1px)` and scales `1.06` — kinetic tension.
- **CTA class pattern:** `group gap-3 pr-2` to accommodate the icon circle.

### 4c. Glass Pill Nav (`.glass-pill`)

A frosted-glass floating navigation bar fixed at the top of the viewport.

- **Position:** `fixed inset-x-0 top-5`, centred with `flex justify-center`, capped at `max-w-xl`.
- **Shape:** `rounded-full`, height `h-14`.
- **Material:** Multi-layer translucency:
  - `backdrop-filter: blur(20px) saturate(180%)`
  - Layered `background`: white at 90% → 60% gradient + 70% base.
  - `::before` pseudo-element adds a radial highlight at 18% from left.
  - Triple box-shadow: white inset top highlight + subtle outer shadow stack.
- **Border:** `1px solid rgb(17 17 17 / 0.08)`.
- **Reduced-transparency fallback:** Solid white, no blur.

### 4d. Eyebrow Badges (`.eyebrow`)

Tiny uppercase labels that introduce sections.

- `rounded-full`, `py-1 px-3`, `10px` font, `letter-spacing: 0.2em`.
- Background: `--surface-soft`. Colour: `--mute`.

### 4e. Chips (HeroUI `<Chip>`)

Used for status indicators ("Live", "Planned", "In development", "Current") and file sizes.

- Size: `sm` exclusively.
- Default HeroUI styling, themed via the CSS custom properties to match the monochrome palette.

### 4f. Accordion (HeroUI `<Accordion>`)

Used on the Changelog page for expandable release notes.

- Default HeroUI compound component pattern (`Accordion.Item` → `Heading` → `Trigger`, `Panel` → `Body`).
- Trigger content: version label + status Chip, with `Accordion.Indicator` on the right.

### 4g. Icon Circles

All icons sit inside a circular container:

- `flex size-11 items-center justify-center rounded-full bg-surface-soft`
- Icon: Phosphor Icons at `size={20} weight="light"`.

### 4h. Image Placeholder

For screens still awaiting real screenshots:

- Dashed border (`border-dashed border-hairline-strong`), `bg-surface-soft`.
- `rounded-[var(--radius-inner)]`, centered icon + label.
- Default aspect: `aspect-video`.

---

## 5. Layout Principles

### Grid & containers

- **Max width:** `max-w-7xl` (1280 px) for full-width sections, `max-w-5xl` for narrower content (downloads), `max-w-4xl` for single-column pages (changelog).
- **Horizontal padding:** `px-6` globally.
- **Section rhythm:** `py-32 md:py-40` for major sections. `py-16` or `py-24` for smaller sections and sub-sections.
- **Hero top padding:** `pt-32 md:pt-40` (accounting for the fixed nav).

### Section anatomy

Every section after the hero follows the same skeleton:

```
<section className="border-t border-hairline">
  <div className="mx-auto max-w-7xl px-6 py-32 md:py-40">
    <span className="eyebrow">Label</span>
    <Reveal as="h2" className="mt-4 max-w-[Xch] text-2xl font-medium tracking-tight md:text-3xl">
      Heading
    </Reveal>
    <div className="mt-12 grid ...">
      {/* Content */}
    </div>
  </div>
</section>
```

- Top border hairline divider. Eyebrow → heading → content grid, spaced `mt-4` / `mt-12`.

### Bento grid (asymmetric)

Cards use a 12-column grid (`md:grid-cols-12`) with asymmetric spans:

- `md:col-span-7` + `md:col-span-5` (alternating)
- `md:col-span-6` + `md:col-span-6` (even pairs)
- Gap: `gap-6` (24 px).

### Hero layout

- Two-column asymmetric split: `md:grid-cols-[1.1fr_0.9fr]` (text-heavy left, visual right).
- Content vertically centred with `items-center`.

---

## 6. Motion & Animation

### Scroll reveal (`<Reveal>` component)

- **Engine:** GSAP + ScrollTrigger.
- **Initial state:** `opacity: 0; transform: translateY(40px); filter: blur(6px)` (set via CSS `[data-reveal]` + GSAP inline).
- **Animation:** `opacity → 1, y → 0, blur → 0` over `0.9s` with `power4.out` easing.
- **Trigger:** `start: "top 85%"`, fires once (`toggleActions: "play none none none"`).
- **Staggering:** Manual `delay` prop, typically `i * 0.05` or `i * 0.06` per item.
- **Reduced motion:** Respects `prefers-reduced-motion: reduce` — elements appear immediately, no animation.

### Smooth scroll (`<SmoothScroll>` component)

- **Engine:** Lenis (`duration: 1.1`, `smoothWheel: true`), synced with GSAP ticker.
- **Reduced motion:** Skipped entirely.

### Micro-interactions

- **Bezel card hover:** `translateY(-2px)` lift, `400ms var(--ease-spring)`.
- **Button icon circle hover:** `translate(2px, -1px) scale(1.06)`, `350ms var(--ease-spring)`.
- **Easing curve:** `--ease-spring: cubic-bezier(0.32, 0.72, 0, 1)` — a snappy, slightly overshooting spring.

---

## 7. Technology & Conventions

| Layer             | Choice                                                       |
|-------------------|--------------------------------------------------------------|
| **Framework**     | Next.js 16 (App Router, React 19, Server Components by default) |
| **Styling**       | Tailwind CSS v4 (CSS-first `@theme inline` config, no `tailwind.config` file) + vanilla CSS custom properties |
| **Component lib** | HeroUI v3 (`@heroui/react`) — `Card`, `Chip`, `Link`, `Accordion`, `buttonVariants` |
| **Icons**         | Phosphor Icons (`@phosphor-icons/react`, SSR imports) — `weight="light"` for decorative, `weight="bold"` for action icons |
| **Animation**     | GSAP (ScrollTrigger) for scroll reveals, Lenis for smooth scrolling, Framer Motion available but not currently used |
| **Font**          | Sora Variable (self-hosted via `next/font/local`)            |
| **PostCSS**       | `@tailwindcss/postcss`                                       |

### File organisation

```
src/
  app/
    globals.css          ← All tokens, utility classes, texture overlays
    layout.tsx           ← Root: font, grain, nav, footer, smooth scroll
    page.tsx             ← Home (hero + bento sections + CTA)
    changelog/page.tsx   ← Changelog (accordion)
    downloads/page.tsx   ← Downloads (hero + build cards + install steps)
  components/
    nav.tsx              ← Fixed glass-pill nav
    footer.tsx           ← Footer with logo, links, tagline
    logo.tsx             ← Logo image wrapper
    reveal.tsx           ← GSAP scroll-reveal wrapper
    smooth-scroll.tsx    ← Lenis smooth scroll initialiser
    image-placeholder.tsx← Dashed placeholder for missing screenshots
  fonts/
    Sora-Variable.ttf
    Sora-VariableItalic.ttf
```

### HeroUI theming

HeroUI's semantic tokens are mapped to Vivianne's palette in `globals.css` `:root`:

```css
--background: var(--canvas);
--foreground: var(--ink);
--accent: var(--ink);
--accent-foreground: var(--canvas);
--default: var(--surface-soft);
--border: var(--hairline);
```

This means HeroUI components inherit the monochrome palette automatically — no per-instance colour overrides needed.

---

## 8. Accessibility

- **Reduced motion:** Both `[data-reveal]` CSS and the `<Reveal>` / `<SmoothScroll>` components check `prefers-reduced-motion: reduce`. When set, elements appear instantly and Lenis is not initialised.
- **Reduced transparency:** The `.glass-pill` falls back to a solid white background when `prefers-reduced-transparency: reduce` is active.
- **Selection contrast:** `::selection` uses full inversion (`--ink` on `--canvas`) for clear legibility.
- **Semantic HTML:** Sections use `<section>`, `<header>`, `<footer>`, `<main>`. Headings follow a single `<h1>` per page with `<h2>` → `<h3>` hierarchy.
- **Antialiasing:** Applied globally via the `antialiased` Tailwind class.

---

## 9. Do / Don't Quick Reference

| ✅ Do                                                        | ❌ Don't                                                      |
|--------------------------------------------------------------|---------------------------------------------------------------|
| Use `--ink` at varying opacities for emphasis hierarchy       | Introduce a new colour hue                                    |
| Wrap scroll-entering content in `<Reveal>`                   | Use `framer-motion` for scroll reveals (GSAP is the standard) |
| Use `.bezel-shell` + `.bezel-core` for cards                 | Use a plain `<div>` with `rounded` and `shadow` for cards     |
| Cap heading width with `max-w-[Xch]`                         | Let headings span the full container                          |
| Use `font-medium` as the heaviest text weight                | Use `font-bold` or `font-semibold`                            |
| Start sections with `.eyebrow` → `<Reveal as="h2">` → grid  | Skip the eyebrow or use a different section intro pattern     |
| Import icons from `@phosphor-icons/react/dist/ssr`           | Use inline SVGs or a different icon library                   |
| Use `weight="light"` for decorative icons, `"bold"` for CTAs | Mix icon weights arbitrarily                                  |
| Separate sections with `border-t border-hairline`            | Use coloured dividers or thick rules                          |
| Keep vertical rhythm at `py-32 md:py-40` for major sections  | Use ad-hoc padding values                                     |
