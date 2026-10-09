# `--json` output (schema 1)

Every `--json` document is one JSON object on one line with `"schema": 1`. Fields are only ever added within a
schema version. Paths are the strings the caller passed, extended with `/` separators. Text is UTF-8; invalid bytes
become U+FFFD.

| Command | Shape |
|---|---|
| `read`, `head`, `tail` | `{schema, files: [{path, total_lines, from, to, remaining, content}]}` — `content` is the shown lines joined with `\n` |
| `stat` | `{schema, files: [{path, type: "file"\|"directory", bytes, lines, language, binary, crlf, modified}]}` — file-only fields are absent for directories |
| `grep` | `{schema, matches: [{path, line, text}], total_matches, files_with_matches, truncated}` — with `-l`/`-c` the array is `files: [{path, count?}]` |
| `glob` | `{schema, paths: [string], total, truncated}` |
| `ls` | `{schema, entries: [string], total, truncated}` — directories end with `/`; recursive listings are flat, in path order |
| `outline` | `{schema, files: [{path, language, lines, symbols: [{line, depth, kind, name}]}]}` |
| `find-def` | `{schema, definitions: [{path, line, kind, text}], total}` |
| `refs` | `{schema, definitions: [...], imports: [...], references: [...], total, truncated}` — each entry `{path, line, text}`; a group is absent if empty |
| `skim` | `{schema, tree: {name, files, lines, languages: {name: lines}, file_list: [{name, lines, language, symbols: [{line, kind, name}]}], dirs: [tree...]}}` — honours `--depth`, ignores `--budget` |

Exit codes are the same as without `--json`: 0 ok, 1 not found / no match, 2 usage error (nothing on stdout, message
on stderr). For `grep` the document is still printed on exit 1, with zero matches; `glob`, `find-def` and `refs` print
nothing when they find nothing.

`kind` values: `class`, `interface`, `struct`, `enum`, `trait`, `impl`, `mod`, `namespace`, `module`, `function`,
`fn`, `func`, `method`, `type`, `const`, `static`, `union`, `macro`, `object`, `record`, `h1`…`h6` (Markdown).
