"""Byte-level data. Any text (or any file at all) is a dataset; there is no tokenizer to train."""

from __future__ import annotations

import urllib.request
from pathlib import Path

import torch

LAB = Path(__file__).resolve().parent.parent
REPO = LAB.parent
CACHE = LAB / "data"

KNOWN = {
    "shakespeare": "https://raw.githubusercontent.com/karpathy/char-rnn/master/data/tinyshakespeare/input.txt",
}


def load_bytes(source: str | None) -> bytes:
    """`source` is a file, a directory (every .md/.txt under it), a name from KNOWN, or None.

    None uses this repository's own Markdown, so the lab runs with no download at all.
    """
    if source in KNOWN:
        path = CACHE / f"{source}.txt"
        if not path.exists():
            CACHE.mkdir(parents=True, exist_ok=True)
            urllib.request.urlretrieve(KNOWN[source], path)
        return path.read_bytes()
    root = Path(source) if source else REPO
    if root.is_file():
        return root.read_bytes()
    files = sorted(
        p
        for p in root.rglob("*")
        if p.suffix in {".md", ".txt"} and p.is_file() and "node_modules" not in p.parts and CACHE not in p.parents
    )
    if not files:
        raise FileNotFoundError(f"no .md or .txt files under {root}")
    return b"\n\n".join(p.read_bytes() for p in files)


class ByteData:
    def __init__(self, source: str | None = None, val_fraction: float = 0.1):
        data = torch.frombuffer(bytearray(load_bytes(source)), dtype=torch.uint8).long()
        split = int(len(data) * (1 - val_fraction))
        self.train, self.val = data[:split], data[split:]

    def batch(self, split: str, batch_size: int, seq_len: int, device: torch.device | str = "cpu") -> torch.Tensor:
        """Random windows of seq_len + 1 bytes: models read the first seq_len and predict the shift."""
        data = self.train if split == "train" else self.val
        starts = torch.randint(len(data) - seq_len - 1, (batch_size,))
        return torch.stack([data[s : s + seq_len + 1] for s in starts]).to(device)

    def __repr__(self) -> str:
        return f"ByteData(train={len(self.train):,} bytes, val={len(self.val):,} bytes)"
