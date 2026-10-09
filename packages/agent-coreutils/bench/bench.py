#!/usr/bin/env python3
"""Wall-clock benchmark: acu against rg/fd on a directory. Warm file cache, process start-up included.

usage: python bench/bench.py <acu-binary> <directory> [runs] [--no-ignore]

--no-ignore turns ignore files off in both tools; use it on corpora whose files a .gitignore would hide
(node_modules, dist). Prints p50/p95 in milliseconds per case. Results are recorded in docs/benchmarks.md.
"""
import os
import shutil
import statistics
import subprocess
import sys
import time


def run(cmd, cwd, runs):
    subprocess.run(cmd, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)  # warm the file cache
    times = []
    for _ in range(runs):
        t = time.perf_counter()
        subprocess.run(cmd, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        times.append((time.perf_counter() - t) * 1000)
    times.sort()
    return statistics.median(times), times[min(len(times) - 1, int(len(times) * 0.95))]


def main():
    positional = [a for a in sys.argv[1:] if not a.startswith("--")]
    noignore = "--no-ignore" in sys.argv
    acu, root = os.path.abspath(positional[0]), positional[1]
    runs = int(positional[2]) if len(positional) > 2 else 15
    rg, fd = shutil.which("rg"), shutil.which("fd")
    count = sum(len(f) for _, _, f in os.walk(root))
    mode = "ignore rules off" if noignore else "ignore rules on"
    print(f"corpus: {root} ({count} files on disk, {mode}); {runs} runs each\n")
    print(f"{'case':44} {'p50 ms':>8} {'p95 ms':>8}")

    ni = ["--no-ignore"] if noignore else []
    cases = [
        ("acu --version (start-up floor)", [acu, "--version"]),
        ("acu grep -F literal", [acu, "grep", "-F", "useState", "-m", "0"] + ni),
        ("rg   -F literal", [rg, "-F", "useState", "--hidden", "-n"] + ni if rg else None),
        ("acu grep regex", [acu, "grep", r"\b[A-Z][a-z]+State\b", "-m", "0"] + ni),
        ("rg   regex", [rg, r"\b[A-Z][a-z]+State\b", "--hidden", "-n"] + ni if rg else None),
        ("acu grep -i regex alt", [acu, "grep", "-i", "error|warning", "-m", "0"] + ni),
        ("rg   -i regex alt", [rg, "-i", "error|warning", "--hidden", "-n"] + ni if rg else None),
        ("acu grep -l (files only)", [acu, "grep", "-l", "TODO|FIXME", "-m", "0"] + ni),
        ("rg   -l", [rg, "-l", "TODO|FIXME", "--hidden"] + ni if rg else None),
        ("acu glob **/*.ts", [acu, "glob", "**/*.ts", "-m", "0"] + ni),
        ("fd   -e ts", [fd, "-e", "ts", "--hidden", "--exclude", ".git"] + ni if fd else None),
        ("acu ls -R", [acu, "ls", "-R", "-m", "0"] + ni),
        ("acu skim", [acu, "skim"] + ni),
        ("acu find-def createAgentStore", [acu, "find-def", "createAgentStore"] + ni),
    ]
    for name, cmd in cases:
        if cmd is None or cmd[0] is None:
            print(f"{name:44} {'(tool not installed)':>17}")
            continue
        p50, p95 = run(cmd, root, runs)
        print(f"{name:44} {p50:8.1f} {p95:8.1f}")


if __name__ == "__main__":
    main()
