# Graphics: posters, covers, social images, cards

Anything with a fixed canvas: a poster, an event flyer, a book or report cover, an Instagram post, a LinkedIn image, a thumbnail, a business card, an invitation, a certificate.

## Make one

Write HTML/CSS for the exact canvas and render it:

- **Image**: `create_document` with `format: "png"`, `html`, and `size: { "width": 1080, "height": 1350 }` (pixels; rendered at 2× for sharpness).
- **Print**: `format: "pdf"`, `html`, and `@page { size: 297mm 420mm; margin: 0 }` in the CSS.

The fonts `'Inter'` and `'Source Serif 4'` are available. Sizes for common canvases are in `layout.md`. Start from `templates/poster.html` or `templates/social.html`.

## Design

- **One message, one focal point.** A headline the size of the canvas allows, a supporting line, the essential details (date, place, link), nothing else.
- **Scale contrast** does the work: a very large headline against small, well-spaced details.
- **A strong grid and margins** (6–8% of the short side). Align everything to it.
- Colour can be bolder here than in documents: one strong background colour or a full-bleed photo, with text in white or ink. Still one accent.
- **Photos**: full-bleed or in a clean frame; text never over a busy area without a solid band or a darkening gradient behind it.
- **Text must be readable at the size it's seen**: a social image is seen at ~400 px wide on a phone; a poster from three metres.
- Generate imagery with `generate_image` when a photo or illustration is needed, then place it in the HTML.
