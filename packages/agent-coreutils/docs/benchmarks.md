# Benchmarks

Measured with `bench/bench.py` (wall clock, process start-up included, one untimed warm-up run, p50/p95 over N runs)
on a Windows 11 laptop, MinGW-w64 GCC 16 `-O2`, 16 logical cores, warm file cache, Defender on. ripgrep is the
`rg` installed on that machine. `fd` was not installed, so there is no `glob` comparison. Raw output is in
`bench/results/`.

**Read these with the caveats below.** One machine, one OS, small corpora. The plan's targets (100k-file trees in
under 100 ms) are **not met** here and no index exists yet.

## 225 files (`packages/gui-next/src`, ignore rules on), 21 runs

| case | acu p50 | rg p50 |
|---|---|---|
| start-up floor (`acu --version`) | 14.8 ms | |
| grep `-F` literal | 23.8 | 31.0 |
| grep regex `\b[A-Z][a-z]+State\b` | 24.1 | 30.8 |
| grep `-i 'error\|warning'` | 25.0 | 32.0 |
| grep `-l` | 23.7 | 31.1 |
| glob `**/*.ts` | 18.8 | |
| ls -R | 20.1 | |
| skim | 26.7 | |
| find-def | 22.9 | |

Almost all of every number is process start-up; the work itself is 4–12 ms.

## 24,311 files (`node_modules/@hugeicons`, ignore rules off), 11 runs

| case | acu p50 | rg p50 |
|---|---|---|
| start-up floor | 15.3 ms | |
| grep `-F` literal | 269.5 | 331.9 |
| grep regex | 294.5 | 353.0 |
| grep `-i` alternation | 355.5 | 469.0 |
| grep `-l` | 381.7 | 376.6 |
| glob `**/*.ts` | 57.8 | |
| ls -R | 58.5 | |
| skim | 417.3 | |
| find-def | 265.1 | |

Several acu p95 values are 2–4 s (see `bench/results/hugeicons-24k.txt`): occasional outliers that rg shows less of
here; not yet explained (suspected antivirus scanning of freshly read files, unconfirmed).

## What this says

* On content search, acu is in the same range as ripgrep on this machine: slightly ahead on literals and regexes,
  level on `-l`. It is not "wide margin" faster, and on 24k files it is 270–420 ms, nowhere near the 40–80 ms
  targets. Per-file open and read cost on Windows dominates, for both tools.
* Directory walking is quick (24k files listed in about 40 ms beyond start-up); `skim` has to read every text file
  and is bounded by the same per-file cost.
* A 170k-file run was started but did not finish in time, so there is no number for that size.
* Linux and macOS have not been measured at all.
* Against "a naive Node.js tool" (a plan goal) nothing was measured.

## Reproduce

```
python bench/bench.py build/acu.exe <directory> 15 [--no-ignore]
```
