#!/usr/bin/env python3
"""Crash safety: kill `acu write` at random moments and check the target is always whole.

After every kill the target file must be exactly the old content or exactly the new content, never a mix or
a truncation, and no temp file may be left behind after a clean run.

usage: python tests/crash_safety.py <acu-binary> [iterations]
"""
import os
import random
import subprocess
import sys
import tempfile
import time

acu = os.path.abspath(sys.argv[1])
iterations = int(sys.argv[2]) if len(sys.argv) > 2 else 500
rng = random.Random(1234)

OLD = (b"old line\n" * 200_000)  # ~1.8 MB so a write takes long enough to interrupt
NEW = (b"NEW LINE!\n" * 300_000)  # ~3 MB

with tempfile.TemporaryDirectory() as d:
    target = os.path.join(d, "target.txt")
    with open(target, "wb") as f:
        f.write(OLD)
    outcomes = {"old": 0, "new": 0}
    bad = 0
    for i in range(iterations):
        p = subprocess.Popen([acu, "write", target, "--raw"], stdin=subprocess.PIPE,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        # feed stdin in pieces and kill at a random point, sometimes before the rename, sometimes after
        try:
            cut = rng.randint(0, len(NEW))
            p.stdin.write(NEW[:cut])
            p.stdin.flush()
            time.sleep(rng.random() * 0.01)
            if rng.random() < 0.7:
                p.kill()
            else:
                p.stdin.write(NEW[cut:])
                p.stdin.close()
                time.sleep(rng.random() * 0.03)
                p.kill()
        except (BrokenPipeError, OSError):
            p.kill()
        p.wait()
        with open(target, "rb") as f:
            data = f.read()
        if data == OLD:
            outcomes["old"] += 1
        elif data == NEW:
            outcomes["new"] += 1
            with open(target, "wb") as f:  # reset for the next round
                f.write(OLD)
        else:
            bad += 1
            print(f"iteration {i}: target is neither old nor new ({len(data)} bytes)")
            with open(target, "wb") as f:
                f.write(OLD)
        for name in os.listdir(d):  # a killed run may leave its temp file; sweep it, but it must never be the target
            if name != "target.txt":
                try:
                    os.remove(os.path.join(d, name))
                except OSError:
                    pass
    # a clean run leaves nothing behind
    subprocess.run([acu, "write", target, "--raw"], input=NEW, check=True)
    leftovers = [n for n in os.listdir(d) if n != "target.txt"]
    print(f"{iterations} kills: old={outcomes['old']} new={outcomes['new']} corrupt={bad}; "
          f"temp files after a clean write: {len(leftovers)}")
    sys.exit(1 if bad or leftovers else 0)
