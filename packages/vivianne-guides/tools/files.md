# Files: read, search, change

## `read`

Read the part you need. A big file read whole costs the context of everything after it.

```json
{ "path": "src/server/auth.ts", "offset": 120, "limit": 60 }
```

`offset` is the first line (1-indexed), `limit` how many. Find the lines with `grep` first, then read around them. PDFs take `pages`: `{ "path": "report.pdf", "pages": "1-5" }`.

## `grep` and `glob`

`grep` takes a regex and an optional directory or file; `glob` takes a pattern with `*` and `?`.

```json
{ "pattern": "fn (load|save)_session", "path": "src-tauri/src" }
```
```json
{ "pattern": "**/*.test.ts", "path": "src" }
```

Search a folder, not the whole project, when you know roughly where it is. A pattern that matches hundreds of lines is the wrong pattern: make it narrower.

## `edit`: one change, exact text

`old_string` must appear **once** in the file, character for character, including indentation. Copy it from a `read`, not from memory.

Works:
```json
{ "path": "src/limits.ts", "old_string": "const MAX_RETRIES = 3\n", "new_string": "const MAX_RETRIES = 5\n" }
```

Fails, because `return null` appears in six places:
```json
{ "path": "src/limits.ts", "old_string": "return null", "new_string": "return undefined" }
```

Fix it by including the lines around the one you mean until the text is unique, or by changing the line that makes it unique.

## `write`: a whole file

`content` is the complete file, never a fragment: whatever you leave out is deleted. For a new file this is right. For an existing file, use `edit`, or `apply_patch` for several changes.

## `apply_patch`: several changes at once

One call, many hunks, many files. The text starts with `*** Begin Patch` and ends with `*** End Patch`.

```json
{ "patchText": "*** Begin Patch\n*** Update File: src/limits.ts\n@@\n-const MAX_RETRIES = 3\n+const MAX_RETRIES = 5\n*** Add File: src/notes.md\n+Retries raised to 5.\n*** End Patch" }
```

## The same change in many files

Not a hundred `edit` calls. Use `run_script` for a pattern over files you can describe in JavaScript, or `run_native` for a large or heavy rewrite: see [scripts](scripts.md).
