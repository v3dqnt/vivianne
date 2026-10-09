# agent-coreutils (`acu`)

Fast, model-friendly file tools for AI agents, in one small native binary written in C++20.
It reads, writes, edits, searches, finds, lists and *skims* a codebase, and prints the least text that answers the
question. It can run as a command, as an MCP server, or as PowerShell cmdlets.

```
acu skim                              the shape of a codebase in one call
acu grep 'TODO|FIXME' src -C 1        search; output is path:line:text, capped, never coloured
acu read src/a.ts --lines 40:80       numbered lines; long files say how to get the rest
acu edit src/a.ts --old 'x = 3' --new 'x = 5'    exact replace; refuses to guess
acu mcp                               every command as an MCP tool over stdio
```

Status: **0.1.0**. Milestones 1–6 and the PowerShell part of 8 of [the plan](../../docs/agent-coreutils-plan.md)
are built and tested on Windows. See [What is not done yet](#what-is-not-done-yet) before relying on it elsewhere.

## Install

```
npm i -g agent-coreutils        # prebuilt binary through optionalDependencies (not published yet)
```

or build from source (CMake 3.20+, a C++20 compiler, no other dependencies):

```
cmake -S . -B build -G Ninja && cmake --build build
build/acu --version
```

Use it from an agent in one of three ways:

* **MCP** (no shell, no per-call process start): `claude mcp add acu -- acu mcp --root .`, or in any MCP client's
  config: `{"command": "acu", "args": ["mcp", "--root", "/path/to/project"]}`. `--root` makes the server's working
  directory the project and refuses paths outside it.
* **Shell**: paste the output of `acu agent-guide` (about 600 tokens) into the agent's instructions or `AGENTS.md`.
* **PowerShell**: `Import-Module ./pwsh/AgentCoreutils.psd1`, then `Search-AcuCode 'TODO' -Path src | Where-Object Line -gt 100`.

## Commands

| Command | What it does |
|---|---|
| `skim [dir] [--depth N] [--budget TOKENS]` | Directories with file/line counts and language mix; files with their top-level symbols; sized to a token budget (default 4,000) by trimming symbols, then files, then depth |
| `outline <file>...` | Symbols with nesting and line numbers |
| `read <file>... [--lines A:B] [--max N]` | `N<TAB>text`; several files under `== path ==` headers; past `--max` (2,000) it says `… 18,410 more lines; use --lines 2001:4000` |
| `head` / `tail [-n N]` | First or last lines, numbered like `read` |
| `stat <path>...` | Size, lines, language, CRLF/BOM, modified time |
| `ls [dir] [-R] [--depth N]` | Directory, or a compact tree |
| `glob <pattern>... [--in DIR]` | `**/*.test.ts`, `src/**/{a,b}.rs`; a pattern with no `/` matches names at any depth |
| `grep <pattern> [paths]` | `-F -i -w -C/-A/-B N --type ts,rs --glob G --max N -l -c`, and `-e A -e B` for several patterns in one pass |
| `find-def <symbol>` / `refs <symbol>` | Where it is defined; every use, ranked definitions → imports → references |
| `write <file>` | Stdin to a file, atomically; keeps the file's line endings and BOM (`--raw` to opt out, `--append`) |
| `edit <file> --old S --new S [--all]` | Exact replace. Not found: writes nothing and shows the closest lines (and says if only whitespace differs). Ambiguous: lists the line numbers. `--edits file.json` applies several, all or nothing |
| `patch [-p N] [--fuzz N]` | Applies a unified diff from stdin; a file with a rejected hunk is left untouched |
| `mv` / `cp` / `rm` | Refuse to overwrite without `--force`; `rm` goes to the OS trash unless `--permanent`, and never removes the current directory |
| `agent-guide` | Prints the short guide for agents |

Every command respects `.gitignore`, `.ignore` and `.acuignore` (`--no-ignore` to disable) and skips binary files.
Most take `--json` (`"schema": 1`, see [docs/json-schema.md](docs/json-schema.md)). `--root DIR` / `ACU_ROOT` refuses
any path that resolves outside `DIR`, including through symlinks and directory junctions.

### Output rules

1. Plain UTF-8, no colour; paths as the caller typed them.
2. Every list is capped; the last line says how many were left out and the flag that shows them.
3. Errors are one line on stderr that say what to do next. Exit codes: `0` ok, `1` not found / no match, `2` usage.
4. Line numbers are 1-based and identical across `read`, `grep`, `outline` and `edit` messages.
5. Lines over 500 characters are cut with `…[+N chars]`.

## How it works

* **Walk**: a parallel directory walk (`FindFirstFileExW` with large fetch on Windows, `readdir` elsewhere) that
  applies compiled ignore rules on the way down, so ignored trees are never entered.
* **Search**: single and multi-literal searches use a substring prefilter on the whole buffer; regexes run on their
  required literal first, then on candidate lines only. The regex engine is a backtracking walk with a visited-state
  bitmap, so it is linear in pattern × line length and cannot blow up (`(a*)*b` on 5,000 `a`s is instant). No
  back-references or look-around (they are reported as errors); `\w \d \s \b` and case folding are ASCII-only.
  `grep` output is identical to ripgrep's on the same files (`tests/diff-vs-rg.sh`; one deliberate difference: `$`
  matches before `\r\n`, as an agent expects).
* **Outline/skim**: a line-based symbol extractor (keywords plus indentation) for TypeScript/JavaScript, Python,
  Rust, Go, Java/Kotlin/Scala, C#, C/C++, Ruby and Markdown. It is a heuristic, not a parser.
* **Writes**: temp file in the same directory, flush to disk, one atomic rename over the target.

## Tests

```
build/acu_tests                                   # 1,600+ checks: regex, glob, ignore, JSON, outlines, every command, MCP, fuzz-lite
bash tests/diff-vs-rg.sh build/acu <dir>          # grep must equal ripgrep (needs rg)
python tests/crash_safety.py build/acu 10000      # kill `acu write` at random moments: target is always old or new, never mixed
python bench/bench.py build/acu <dir> [runs] [--no-ignore]
```

Measured numbers and their caveats are in [docs/benchmarks.md](docs/benchmarks.md).

## What is not done yet

Honest list against the plan:

* **Only built and run on Windows (x64, MinGW-w64 GCC 16).** The POSIX paths (`readdir`, `fsync` + `rename`, XDG/macOS
  trash) are written but have not been compiled or run; expect small fixes on first Linux/macOS build. There is no CI
  history yet (`.github/workflows` are written, never run) and no published npm packages, Homebrew tap, Scoop bucket
  or install script (`packaging/` has templates).
* **No Vectorscan, RE2, simdutf or Tree-sitter.** The plan used them; this version has its own regex engine and
  symbol extractor so the project builds with nothing but a compiler. Swapping them in behind `Regex` and
  `extractSymbols` is the intended path if the numbers call for it.
* **No index or daemon (milestone 7).** Without it, 100k-file trees are walked on every call.
* **No libFuzzer targets**; `tests/tests.cpp` has seeded random-input loops for regex, glob, edit and patch instead.
* Windows start-up for any process is 20–30 ms on the machine it was measured on, which dominates small commands.
  Use `acu mcp` to pay it once.
* UTF-16 files can be read, searched and outlined, but not edited or overwritten.

## License

MIT, see [LICENSE](LICENSE).
