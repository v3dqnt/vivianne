# Word documents

Make a .docx when the reader will **edit** it: drafts, contracts, templates, documents that go through review. Otherwise make a PDF.

`create_document` with `format: "docx"` takes the same `blocks`, `theme`, `title`, `cover` as PDF. What makes it good to work in:

- **Real styles**: Title, Heading 1–3, Caption. Word's navigation pane, table of contents and restyling all work. Never fake a heading with bold text.
- **Real lists**: bullets and numbering Word understands, so the user can keep typing.
- **Quiet tables**: header rule, hairlines, numbers right-aligned.
- Charts become images (Word can't hold an SVG chart editably); if the user needs to change the data, put it in a table too or attach an Excel file.
- Fonts: Aptos (or Georgia for serif themes), so it looks right on any Office install.

To change an existing Word file, don't regenerate it: `inspect_document` then `edit_document`, which keeps the user's own formatting.
