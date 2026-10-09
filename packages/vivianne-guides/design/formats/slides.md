# Slides

A deck is a sequence of single ideas. Each slide should be understood in five seconds.

## Make one

`create_document` with `format: "pptx"` (editable in PowerPoint/Keynote/Google Slides) or `format: "pdf"` with `slides` (for sending and presenting from a PDF). Same slide description for both.

```json
{ "format": "pptx", "path": "Q4 Review.pptx", "theme": "modern",
  "slides": [
    { "title": "Q4 Business Review", "subtitle": "Board meeting · October 2026" },
    { "layout": "section", "title": "Results", "subtitle": "What happened this quarter" },
    { "title": "Revenue grew 18%, led by EMEA", "stats": [{ "label": "Revenue", "value": "$4.2M", "note": "+18% YoY" }] },
    { "title": "Growth held every quarter", "chart": { "kind": "line", "labels": ["Q1","Q2","Q3","Q4"], "series": [...] } },
    { "title": "What's working, what to watch", "left": { "heading": "Working", "bullets": [...] }, "right": { "heading": "Watch", "bullets": [...] } },
    { "quote": "…", "by": "…" },
    { "layout": "closing", "title": "Thank you", "subtitle": "Questions?" }
  ] }
```

Layouts (chosen automatically from the fields, or set `layout`): `title`, `section` (numbered 01, 02…), `bullets`, `two_column`, `image`, `quote`, `stats`, `table`, `chart`, `closing`. Any slide can have a `subtitle` (a lede line under the title).

## Rules

- **The title is the takeaway**, in sentence case: "Churn halved after onboarding changes", not "Churn".
- ≤ 5 points per slide, ≤ ~10 words each. If you need more, split the slide.
- One chart or one table per slide, with a title that states the finding.
- Big numbers get a `stats` slide, not a bullet.
- Use `section` slides to structure decks longer than ~8 slides.
- Open with a title slide; close with a single clear ask or summary.
- Dark theme (`midnight`) for keynote-style screen presentations; light for documents that will be read.
- Images: real screenshots or photos, large, never clip art.
- Speaker detail goes in `notes`, not on the slide.

## Free-form

For a slide design the layouts can't express (a full-bleed photo with overlaid title, a timeline, a matrix), make a PDF deck from your own HTML: each slide a `<section>` of 338.67 × 190.5 mm with `break-after: page`, and `@page { size: 338.67mm 190.5mm; margin: 0 }`.
