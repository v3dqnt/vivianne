# PDF documents

Reports, briefs, proposals, letters, one-pagers, CVs, invoices, research papers. A PDF is final: it should look exactly as intended on any machine. Vivianne prints PDFs from HTML/CSS with the system browser, with Inter and Source Serif 4 embedded.

## Two ways to make one

**1. Blocks** (most documents) — `create_document` with `format: "pdf"`, a `theme`, and `blocks`. The house design is applied: type scale, tables, numbers, quotes, running heads, page numbers.

```json
{ "format": "pdf", "path": "Q4 Board Report.pdf", "theme": "modern",
  "title": "Q4 Business Review", "subtitle": "Prepared for the board",
  "eyebrow": "Board report", "author": "Finance", "date": "October 2026", "cover": true,
  "blocks": [
    { "type": "heading", "text": "Executive summary" },
    { "type": "paragraph", "text": "Revenue grew **18%** to $4.2M …" },
    { "type": "metrics", "items": [{ "label": "Revenue", "value": "$4.2M", "note": "+18% YoY" }] },
    { "type": "chart", "kind": "bar", "title": "Revenue grew every quarter ($k)", "labels": ["Q1","Q2","Q3","Q4"],
      "series": [{ "name": "2026", "values": [960,1120,1310,1480] }, { "name": "2025", "values": [820,940,1105,1230] }] },
    { "type": "table", "caption": "Revenue by region", "rows": [["Region","Q3","Q4"],["EMEA",1240,1480]] },
    { "type": "callout", "tone": "warning", "title": "Risk", "text": "…" },
    { "type": "quote", "text": "…", "by": "Head of Ops, a customer" }
  ] }
```

Blocks: `heading` (level 1–3), `paragraph`, `bullets` / `numbered` (items can nest: `{text, items}`), `table` (`rows`, `header`, `caption`, `align`), `image` (`path`, `caption`, `width` %), `quote`, `callout` (`tone`: info/success/warning/danger), `code`, `metrics` (`items`: label, value, note), `chart` (`kind`: bar/hbar/line/area/pie/donut), `divider`, `page_break`. Inline: `**bold**`, `*italic*`, `` `code` ``, `[link](url)`.

Options: `page_size` (`A4`, `letter`, `A3`, `A5`, `legal`) or `page: {width_mm, height_mm}`, `orientation: "landscape"`, `footer` (running head text), `cover`, `eyebrow`.

**2. Your own HTML** — `create_document` with `format: "pdf"` and `html`. Use it whenever the blocks can't express the design: a CV with a sidebar, an invoice, a certificate, a menu, a programme, multi-column layouts, anything. A fragment gets the house stylesheet (fonts, type scale, `.eyebrow`, `.metrics`, `.callout`, tables), so you only write what's different. A full document (`<!doctype html>`) is used as is (fonts still available as `'Inter'` and `'Source Serif 4'`). Set the page with CSS: `@page { size: 210mm 297mm; margin: 20mm }`. Start from a file in `templates/`.

## Choosing a theme

- `modern` — default for business and product work.
- `editorial` — long reads, essays, research, letters.
- `classic` — formal, finance, legal, academic.
- `warm` — culture, hospitality, personal.
- `minimal` — CVs, portfolios, design-led work.
- `forest` — sustainability, health, outdoors.
- `{ "preset": "modern", "accent": "#0A7A5A" }` — a brand colour on any preset.

## Patterns by document

- **Report**: cover for anything over ~4 pages; executive summary first with 3–4 key `metrics`; one idea per section; charts with finding titles; appendix for detail.
- **Letter**: no cover; sender block top-left or right, date, recipient, salutation; one page; generous margins; `editorial` or `classic`.
- **One-pager**: a title that states the offer; 3 sections max; one visual; contact at the bottom.
- **CV**: name large, one line of who they are, contact line; experience as role · company · dates with 2–3 achievement bullets each; skills compact; one page. Use `templates/cv.html`.
- **Invoice**: issuer and client blocks, invoice number and dates aligned right, line items table, totals right-aligned with the total in bold above a rule, payment details small at the bottom. Use `templates/invoice.html`.
