# Design System: Vivianne Site
**Project ID:** vivianne-site

## 1. Visual Theme & Atmosphere
The atmosphere is quietly confident, engineered, and precise, resembling milled aluminum and frosted glass. It feels like a piece of premium hardware rather than a flat digital poster. The design density is exceptionally airy, utilizing generous whitespace and unhurried, spring-eased motion. A subtle, fixed full-screen film grain overlay gives the white canvas a photographic and tactile physical quality. The aesthetic philosophy revolves around extreme restraint: a predominantly monochrome "ink and canvas" palette is used, where color is withheld almost entirely, saving the single sky-blue-to-brushed-silver accent for the logo to make it feel earned rather than decorative.

## 2. Color Palette & Roles
* **Canvas (Pure White)** (`#ffffff`) - Serves as the base page background, providing extreme clarity.
* **Surface Raised (Pure White)** (`#ffffff`) - Used for the inner core of cards/containers that sit "above" the canvas.
* **Ink (Near-Black)** (`#111111`) - Primary text color for maximum legibility and strong accents.
* **Ink Strong (Softened Ink)** (`rgba(17, 17, 17, 0.82)`) - Used for secondary emphasis.
* **Body (Warm Mid-Gray)** (`#6b6b6b`) - Used for body and paragraph text to reduce eye strain.
* **Mute (Translucent Ink)** (`rgba(17, 17, 17, 0.4)`) - De-emphasized labels, metadata, and eyebrow text.
* **Hairline (Light Gray)** (`#e8e8e8`) - Default borders and structural dividers; barely visible.
* **Hairline Strong (Translucent Ink)** (`rgba(17, 17, 17, 0.38)`) - Stronger borders, such as dashed placeholder outlines.
* **Surface Soft (Subtle Tint)** (`rgba(17, 17, 17, 0.04)`) - Subtle tinted backgrounds for icon circles and eyebrow badges.
* **Surface Subtle (Lightest Tint)** (`rgba(17, 17, 17, 0.03)`) - The lightest background tint, used sparingly for outer bezel shells.
* **Logo Ring (Sky Blue to Silver Gradient)** (`linear-gradient(135deg, #4f8fe0 0%, #b7c2cc 55%, #e9edf0 100%)`) - Reserved exclusively for the logo.

## 3. Typography Rules
The system uses the **Sora** variable font (with fallbacks to system-ui and sans-serif). Font weights are strictly limited to medium (500) and normal (400) — no bold, semibold, or light weights are used on text, establishing hierarchy through size and color instead. Headers are large and utilize tight letter-spacing with tight line height. Body text is set with a relaxed line height for comfortable reading and normal tracking. Tiny "eyebrow" text is uppercase, medium weight, and uses extreme letter-spacing.

## 4. Component Stylings
* **Buttons:** Pill-shaped (fully rounded at `28px`). Assigned a near-black background with a white foreground. Buttons often feature a trailing icon inside a distinct circular chip that translates diagonally and scales up slightly on hover, introducing kinetic tension.
* **Cards/Containers:** Employs a double-bezel nested architecture reading as machined hardware. The outer tray ("bezel-shell") has a generous `28px` corner roundness, pale tinted background, and a `1px` inset shadow rim. The inner core ("bezel-core") sits slightly less rounded (`18px`), with a white surface, layered floating shadows, and a `1px` white inset highlight on top. On hover, the core lifts smoothly without color changes.
* **Inputs/Forms:** Subtle tinted backgrounds (`rgba(17, 17, 17, 0.04)`) with barely visible light gray borders (`#e8e8e8`). Pill-shaped or softly rounded corners (`18px`). Focus state relies on a strong near-black (`#111111`) outline.
* **Glass Pill (Navigation):** Floating bar shaped as a pill, built with heavily blurred frosted glass (`backdrop-filter: blur(20px) saturate(180%)`), multi-layer white translucency, an inner radial highlight, and a subtle outer shadow stack.

## 5. Layout Principles
The layout prioritizes an airy, unhurried whitespace strategy. Major sections breathe heavily with massive vertical padding (128px-160px). Content width is strictly constrained, capping at `1280px` for full-width sections, and text paragraphs are deliberately constrained to a comfortable reading measure (max 48 characters for body text, 18-30 characters for headings). Grid alignment uses asymmetric 12-column bento grids (e.g., alternating 7-col and 5-col cards, or text-heavy 1.1fr / 0.9fr visual splits in heroes) with a standard `24px` gap, and a global horizontal margin of `24px`.
