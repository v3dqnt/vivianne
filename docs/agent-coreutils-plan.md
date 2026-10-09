# agent-coreutils — plan

A fast, open-source command-line toolkit, written in C++, that gives any AI agent the file tools it needs:
read, write, edit, search, find, list and skim a codebase. An agent installs it with one command and then
uses its commands instead of its built-in tools.

Working name `agent-coreutils` (free on npm as of 2026-10-07), binary `acu`. Rename freely.

This document is the brief for building it. It is self-contained: nothing here depends on Vivianne.

---

## 1. Goals

1. **Faster than an agent's built-in tools**, by a wide margin, on real repositories.
2. **Output made for models.** Every command prints the least text that answers the question: line numbers
   where they help, truncation with a note saying how to get the rest, no ANSI colour, no progress bars.
   This matters as much as speed: a fast tool that floods the context window is a slow agent.
3. **Easy to install anywhere** an agent runs: `npm i -g agent-coreutils` (or `npx`), with prebuilt binaries
   for Windows, macOS and Linux on x64 and arm64. No compiler or runtime needed on the user's machine.
4. **Safe by default.** Atomic writes, edits that refuse to guess, optional confinement to a project root.
5. **Open source** under MIT or Apache-2.0, with the benchmarks published and reproducible.

### Non-goals (v1)

- Running shell commands, a language server, or anything networked.
- Replacing git. Reading `.gitignore` is in scope; `git status` and friends are not.

---

## 2. Performance targets, stated honestly

"Instant" needs a definition someone can check. Two facts shape the targets:

- **Cold reads are bounded by the disk and the operating system.** The first walk of a 100k-file tree on a
  cold cache cannot finish in 100 ms on any machine; what the tool controls is everything after the bytes
  arrive.
- **Process start-up is a floor.** A static C++ binary starts in about 1–3 ms on Linux and macOS, and
  5–15 ms on Windows. Every command pays it once.

The targets below are on a warm OS file cache, on a 2023-class laptop with an NVMe SSD. Each one is a
benchmark in CI (section 9), compared against `ripgrep`/`fd` and against a naive Node.js tool.

| Operation | Repository | Target (p50) |
|---|---|---|
| `acu skim` (structure + symbols) | 10k files / 1.5M lines (e.g. a mid-size monorepo) | < 100 ms |
| `acu skim` | 100k files (e.g. the Linux kernel) | < 100 ms with the index (section 6), < 1 s without |
| `acu grep` literal | 10k files | < 40 ms |
| `acu grep` regex | 10k files | < 80 ms |
| `acu glob` / `acu ls -R` | 100k files | < 60 ms |
| `acu read` one file | 1 MB | < 5 ms plus start-up |
| `acu write` / `acu edit` | 1 MB | < 5 ms plus start-up, atomic |

For anything under ~10k files, the targets hold with no index at all.

---

## 3. Commands

Every command:
- takes paths relative to the current directory or absolute;
- respects `.gitignore`, `.ignore` and `.acuignore` unless `--no-ignore` is passed;
- skips binary files in text operations (detected by a NUL in the first 8 KB);
- has a `--json` mode with a stable, documented schema, for agents that would rather parse.

### Reading

| Command | What it does | Model-facing details |
|---|---|---|
| `acu read <file> [--lines A:B] [--max N]` | Print a file, or a range of lines | `cat -n` style line numbers; past `--max` (default 2,000 lines) it stops and says `… 18,410 more lines; use --lines 2001:4000` |
| `acu read <f1> <f2> …` | Several files in one call | Each under a `== path ==` header; one process instead of N tool calls |
| `acu head` / `acu tail` | First or last N lines | Same numbering, so line numbers match `read` |
| `acu stat <path…>` | Size, line count, modified time, language, binary or not | One line per path |

### Writing and editing

| Command | What it does | Safety |
|---|---|---|
| `acu write <file>` | Write stdin to a file | Writes a temp file, `fsync`s it, then renames it over the target, so a crash never leaves half a file. Keeps line endings and BOM unless told otherwise |
| `acu edit <file> --old <s> --new <s> [--all]` | Exact-string replace | Fails, writing nothing, if `--old` isn't found, or is found more than once without `--all`. The error shows the near-misses (closest matches by line), so the agent can correct itself in one step |
| `acu edit <file> --edits edits.json` | Several replacements, all or nothing | Validates every edit before writing any |
| `acu patch [-p N]` | Apply a unified diff from stdin | Fuzz-free by default; `--fuzz N` to allow drift; reports each rejected hunk |
| `acu mv` / `acu cp` / `acu rm` | Move, copy, delete | `rm` goes to the OS trash by default; `--permanent` to delete outright |

Optional, for agents run on untrusted prompts: `--root <dir>` (or the `ACU_ROOT` environment variable)
refuses any path that resolves outside that directory, symlinks included.

### Searching

| Command | What it does |
|---|---|
| `acu grep <pattern> [paths] [-F] [-i] [-w] [-C N] [--type ts,rs] [--max N]` | Search file contents. Output is `path:line:text`, grouped by file, capped by `--max` (default 200 matches) with a count of what was left out. `-l` for file names only, `-c` for counts |
| `acu grep -e A -e B …` | Several patterns in one pass (multi-pattern matching, not N passes) |
| `acu glob <pattern…>` | Find files by name: `**/*.test.ts`, `src/**/{a,b}.rs` |
| `acu ls [dir] [-R] [--depth N]` | List a directory; `-R` prints a compact tree, with the same caps |
| `acu find-def <symbol>` | Where a function, class, type or constant is defined (uses the symbol table from `skim`) |
| `acu refs <symbol>` | Word-boundary search for a symbol, ranked: definitions first, then the files that import it |

### Skimming (the headline feature)

`acu skim [dir] [--depth N] [--budget TOKENS]` gives an agent the shape of a codebase in one call:

```
src/ (412 files, 61k lines)  TypeScript 88%, CSS 9%
  modules/ai/ (54 files)
    store/agentStore.ts (713)  createAgentStore · sendMessage · abort · steer · refreshQueues
    components/Composer.tsx (535)  Composer · flattenTree · formatFilesForPrompt
  …
packages/gui/src-tauri/src/ (210 files, 98k lines)  Rust
    agent/loop_core.rs (1,480)  run_agent_loop · drain_queue · system_reminder · emit_abort
```

- **Contents:** per directory, file and line counts plus the language mix. Per file, the line count and the
  top-level symbols in source order.
- **Budget:** `--budget` (default about 4k tokens) decides how deep and how wide it goes. Large directories
  are summarised ("37 more files: *.test.ts") instead of listed.
- **Symbols** come from Tree-sitter grammars compiled into the binary. Start with TypeScript/JavaScript,
  Python, Rust, Go, Java, C/C++, C# and Ruby. Any other language falls back to a tuned regex outline.
- `acu outline <file>` is the same for one file, with nesting (methods under classes) and line numbers.

---

## 4. Architecture

- **Language and build:** C++20, built with CMake. One static binary per platform, with no runtime
  dependencies (static libstdc++ on Linux, a static CRT on Windows).
- **Concurrency:** a work-stealing thread pool sized to the cores, shared by the directory walk and the
  per-file work.
- **Directory walk:**
  - Linux: `getdents64`. macOS: `getattrlistbulk`. Windows: `FindFirstFileExW` with `FIND_FIRST_EX_LARGE_FETCH`
    and `FindExInfoBasic`, or `NtQueryDirectoryFile`.
  - Ignore rules are compiled once into a matcher (not re-parsed per directory) and applied while walking,
    so ignored trees like `node_modules` and `target` are never entered.
- **Reading files:**
  - Small files are read in one call; large files are memory-mapped.
  - UTF-8 is validated with SIMD (simdutf); UTF-16 files are detected by their BOM and converted.
- **Search:**
  - Literals use a SIMD `memchr`/`memmem` (as `ripgrep` does) with a rare-byte prefilter.
  - Regexes use Vectorscan (the portable, BSD-licensed fork of Hyperscan), with RE2 as the fallback where
    Vectorscan can't build a pattern (backreferences are not supported; say so).
  - Several patterns compile into one database and run in one pass.
- **Output:**
  - One buffered writer per thread; results are merged in path order, so output is deterministic.
  - Nothing is printed until a file's results are complete, so files never interleave.
- **Platforms:**
  - **Windows:** long paths (`\\?\`), CRLF kept on write, and `ReplaceFileW` for atomic replace.
  - **macOS:** `F_FULLFSYNC` where durability matters.

### Dependencies (all permissively licensed, vendored or fetched with CMake)

| Library | Why | License |
|---|---|---|
| Vectorscan | Fast multi-pattern regex | BSD-3 |
| RE2 | Fallback regex engine | BSD-3 |
| simdutf | UTF-8/16 validation and conversion | Apache-2.0 / MIT |
| tree-sitter + grammars | Symbols for `skim` / `outline` / `find-def` | MIT |
| CLI11 | Argument parsing | BSD-3 |
| fmt | Fast formatting | MIT |
| (tests) Catch2, Google Benchmark | Tests and benchmarks | BSL-1.0 / Apache-2.0 |

---

## 5. Output rules (for models)

These are part of the contract and are tested.

1. Plain UTF-8, no colour, and paths relative to the current directory (absolute only if given absolute).
2. Every list is capped. The last line says how many were left out and the exact flag that shows them.
3. Errors go to stderr, one line, saying what to do next, with exit code 2 for usage errors and 1 for "not
   found / no match". Example: `edit: --old text found 3 times in src/a.ts (lines 12, 40, 77); pass --all or
   include more surrounding text`.
4. Line numbers are 1-based everywhere, and the same in `read`, `grep`, `outline` and `edit` messages.
5. Long lines are cut at 500 characters with `…[+1,204 chars]`, so minified files can't swamp a result.
6. `--json` output has a versioned schema (`"schema": 1`) published in the repo.

---

## 6. Optional index, for 100 ms on very large repositories

- **The index:** `acu index` writes `.acu/index` at the project root. It holds the file list with mtime and
  size, per-file symbol tables, and a trigram index for `grep` candidates, as in Zoekt or Google Code Search.
- **Freshness:** every command validates the index against a quick stat walk, re-indexes only changed files
  and falls back to a full scan if the index is missing or stale. Results are never wrong because of the
  index; at worst, they're slower.
- **Daemon:** `acu daemon` is optional. It keeps the index in memory, updated by a file watcher (inotify,
  FSEvents, ReadDirectoryChangesW). Commands talk to it over a local socket or named pipe when it's running.
  This is what makes `skim` and `grep` hold their targets on 100k-file trees.
- The index is off by default and is never required.

---

## 7. How agents use it

- **Install:**
  - `npm i -g agent-coreutils` (or `npx agent-coreutils …`).
  - Also: a Homebrew tap, a Scoop manifest, and `curl -fsSL …/install.sh | sh` for machines without Node.
- **`acu agent-guide`:** prints a short, model-oriented guide (about 600 tokens): which command replaces
  which built-in tool, the output rules, and three examples. Agents paste it into their system prompt or
  `AGENTS.md`. Keep it in the repo as `AGENT_GUIDE.md`, generated from the same source.
- **MCP server mode:**
  - `acu mcp` serves every command as an MCP tool over stdio, with JSON schemas.
  - Agents that speak MCP (Claude Code, Cursor, Vivianne and others) can use it with no shell at all, and
    without paying process start-up on each call.
  - This is likely the main way it gets used.
- **PowerShell:** the binary works from any shell. A small PowerShell module (`AgentCoreutils.psm1`) adds
  real cmdlets (`Get-AcuFile`, `Search-AcuCode`, `Edit-AcuFile`…) that return objects, for people who want
  them in PowerShell pipelines.

---

## 8. Repository layout

```
agent-coreutils/
  CMakeLists.txt, CMakePresets.json
  src/
    main.cpp                 command dispatch
    cmd/                     one file per command (read.cpp, grep.cpp, skim.cpp, edit.cpp …)
    core/                    walker, ignore matcher, file I/O, atomic write, thread pool, output writer
    search/                  literal, vectorscan, re2 backends
    lang/                    language detection, tree-sitter outlines, regex fallbacks
    index/                   on-disk index, daemon, watcher
    mcp/                     MCP stdio server
  tests/                     Catch2 unit tests + golden-output tests (fixtures/ repos)
  bench/                     Google Benchmark + hyperfine scripts, pinned corpora
  npm/
    agent-coreutils/         the JS shim: picks the platform package, execs the binary
    @agent-coreutils/win32-x64, darwin-arm64, linux-x64-gnu, linux-x64-musl, … (binary-only packages)
  pwsh/                      PowerShell module
  docs/                      AGENT_GUIDE.md, json-schema/, benchmarks.md
  .github/workflows/         ci.yml, release.yml, bench.yml
```

**npm distribution:**
- Use the esbuild/Biome pattern: the main package lists the per-platform packages as
  `optionalDependencies`, each with `os` and `cpu` fields, so npm downloads only the one that fits.
- The shim resolves that package's binary and `exec`s it (`child_process.spawnSync` with `stdio:
  'inherit'`).
- A `postinstall` fallback downloads from GitHub Releases if optional dependencies were skipped.

---

## 9. Testing and benchmarks

- **Golden tests:** each command runs against fixture repositories (mixed languages, CRLF and LF, BOMs,
  binaries, symlinks, deep ignores, Unicode paths, files over 4 GB on 64-bit), comparing against expected
  output files.
- **Differential tests:**
  - `acu grep` must find exactly what `rg` finds, over a corpus of patterns.
  - `acu glob` must match exactly what `fd` matches.
  - Differences are failures unless documented.
- **Fuzzing:** libFuzzer on the ignore-rule parser, glob compiler, `edit` (old/new/file triples) and the
  patch applier.
- **Crash safety:** kill `acu write` at random points in a loop and check the target is always either the
  old or the new content, never a mix.
- **Benchmarks:**
  - `bench.yml` runs nightly and on release, on pinned corpora: a 10k-file TypeScript monorepo, the Linux
    kernel, and a 1M-line single file.
  - It records p50/p95 against `rg`, `fd`, `cat` and a Node.js baseline into `docs/benchmarks.md`.
  - A pull request that regresses a target by more than 10% fails.
- **CI matrix:** Windows (MSVC and clang-cl), macOS (arm64, x64) and Linux (glibc, musl; x64, arm64), plus
  ASan/UBSan and TSan builds on Linux.

---

## 10. Milestones

| # | Scope | Done when |
|---|---|---|
| 1 | Skeleton: CMake, CLI parsing, output writer, thread pool, walker + ignore rules, `ls`, `glob`, `stat` | Golden tests pass on all three OSes; `glob` matches `fd` on the corpus |
| 2 | `read`, `head`, `tail`, `write` (atomic), `edit` (incl. multi-edit and near-miss errors), `mv`/`cp`/`rm` | Crash-safety test passes 10,000 iterations |
| 3 | `grep`: literal + Vectorscan + RE2 fallback, multi-pattern, context, caps | Differential test vs `rg` is clean; 10k-file target met |
| 4 | `skim`, `outline`, `find-def`, `refs` with Tree-sitter for the first 8 languages | `skim` < 100 ms on the 10k-file corpus within its token budget |
| 5 | npm packaging with platform packages, Homebrew, Scoop, install script, `agent-guide` | `npx agent-coreutils skim` works on a clean machine per OS |
| 6 | `acu mcp` server mode | Works as an MCP server in Claude Code and Vivianne; schema tests pass |
| 7 | Index + daemon | 100k-file targets met; stale-index tests can never produce a wrong result |
| 8 | PowerShell module, `patch`, 1.0 docs, public benchmark page | 1.0 release |

Milestones 1 to 5 are a usable 0.x release. MCP mode (6) is the step most likely to get it adopted.

---

## 11. Open questions for whoever builds it

1. **License:** MIT or Apache-2.0? Apache-2.0 adds a patent grant; MIT is simpler. Both are compatible with
   every dependency above.
2. **Name and npm scope:** keep `agent-coreutils`, or register an org scope (`@acu/…`) for the platform
   packages before publishing.
3. **Tree-sitter grammars:** bundling all eight adds about 6–10 MB to the binary. Alternative: ship the
   regex outlines in the base binary and load grammars as an optional download.
4. **Trash vs delete:** keep `rm` going to the trash by default (safer for agents), or match Unix `rm`?
