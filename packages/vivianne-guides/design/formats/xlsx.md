# Spreadsheets

A spreadsheet is a tool, not a picture. It's well designed when it's easy to read, hard to break, and still works when the user changes a number.

`create_document` with `format: "xlsx"`:

```json
{ "format": "xlsx", "path": "Sales.xlsx", "theme": "modern", "sheets": [{
  "name": "Sales",
  "columns": [{ "width": 22 }, { "format": "currency" }, { "format": "currency" }, { "format": "percent" }],
  "rows": [["Region", "Q3", "Q4", "Growth"], ["EMEA", 1240, 1480, "=C2/B2-1"]],
  "total_row": { "label": "Total", "columns": { "B": "sum", "C": "sum" } },
  "charts": [{ "type": "column", "title": "Revenue by region", "categories": "A", "values": ["B", "C"], "anchor": "F2" }]
}] }
```

- **Formulas, not values**, for anything derived: totals, growth, shares. Start a string with `=`.
- **Number formats** per column: `currency` (`currency: "€"` on the sheet to change the symbol), `percent`, `number`, `integer`, `date`.
- **Header row**: frozen and filterable by default.
- **One table per sheet**, starting at A1. Inputs and assumptions on their own sheet, clearly labelled.
- **Charts are native Excel charts** over the sheet's data, so they update when the data does.
- Cell-level styling when it means something: `{ "value": 12, "bold": true, "fill": "#FFF4E5" }` — e.g. inputs the user should change.
- No merged cells, no colours for decoration, no blank rows in the middle of data.
