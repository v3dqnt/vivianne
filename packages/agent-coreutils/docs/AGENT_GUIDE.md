# Using `acu` (agent-coreutils)

`acu` is a set of fast file commands made for AI agents. Use it instead of your built-in read, write, edit,
search, find and list tools. Output is plain text with 1-based line numbers, never coloured, and always capped:
the last line of a capped result says how many items were left out and which flag shows them.

## Which command replaces which tool

| You want to | Run |
|---|---|
| Understand a codebase | `acu skim [dir]` (directories, sizes, languages, top-level symbols, within about 4k tokens) |
| See one file's structure | `acu outline <file>` |
| Read a file | `acu read <file>` or `acu read a.ts b.ts` (several at once); `--lines 40:80` for a range |
| Search contents | `acu grep <pattern> [paths]` (`-F` literal, `-i`, `-w`, `-C 3`, `--type ts`, `-l` files only) |
| Find files by name | `acu glob '**/*.test.ts'` |
| List a directory | `acu ls [dir]` or `acu ls -R --depth 2` |
| Find a definition | `acu find-def <symbol>`, then `acu refs <symbol>` for its uses |
| Create or replace a file | `acu write <file>` with the content on stdin (atomic; keeps line endings) |
| Change part of a file | `acu edit <file> --old TEXT --new TEXT` |
| Apply a diff | `acu patch` with the diff on stdin |
| Move, copy, delete | `acu mv`, `acu cp`, `acu rm` (`rm` goes to the trash; `--permanent` to delete) |

## Rules worth knowing

- `.gitignore`, `.ignore` and `.acuignore` are respected; `--no-ignore` turns that off. Binary files are skipped.
- `acu edit` never guesses. If `--old` is missing it writes nothing and shows the closest lines; if it matches
  more than once it lists the line numbers: add surrounding text to make it unique, or pass `--all`.
- Line numbers are the same in `read`, `grep`, `outline` and `edit` messages, so you can chain them.
- Exit codes: 0 ok, 1 not found or no match, 2 usage error. Errors are one line on stderr that says what to do.
- `--json` gives machine-readable output (`"schema": 1`) for most commands.
- Long lines are cut at 500 characters with `…[+N chars]`.
- `--root DIR` (or `ACU_ROOT`) refuses any path outside DIR, symlinks included.

## Examples

```
acu skim --depth 2
acu grep 'TODO|FIXME' src --type ts -C 1
printf 'export const x = 1;\n' | acu write src/x.ts
acu edit src/a.ts --old 'retries = 3' --new 'retries = 5'
```
