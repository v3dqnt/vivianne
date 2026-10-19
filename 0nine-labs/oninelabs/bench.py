"""Train every member of the family on the same data, budget and width, and compare them.

    python -m oninelabs.bench --steps 500
    python -m oninelabs.bench --models ssm looped --data shakespeare --steps 3000

Prints a Markdown table and writes runs/bench/results.json. Also measures what only some
members can do: quality at different loop counts for Viv-L and Viv-H (test-time compute),
generation speed in bytes per second, and how many bytes Viv-H's drafter
saves per backbone pass.
"""

from __future__ import annotations

import argparse
import json
import time

import torch

from .data import LAB, ByteData
from .models import FAMILY, ByteModel
from .train import config_args, config_from, evaluate, pick_device, train


@torch.no_grad()
def generation_speed(model: ByteModel, device: torch.device, n: int = 128) -> float:
    model.eval()
    x = torch.zeros(1, 16, dtype=torch.long, device=device)
    start = time.perf_counter()
    model.generate(x, n)
    return n / (time.perf_counter() - start)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--models", nargs="+", choices=FAMILY, default=list(FAMILY))
    parser.add_argument("--data", default=None)
    parser.add_argument("--steps", type=int, default=500)
    parser.add_argument("--batch-size", type=int, default=16)
    parser.add_argument("--lr", type=float, default=1e-3)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--seed", type=int, default=0)
    config_args(parser)
    args = parser.parse_args()

    device = pick_device(args.device)
    cfg = config_from(args)
    data = ByteData(args.data)
    results = []
    for name in args.models:
        model, result = train(
            name, cfg, data, args.steps, args.batch_size, args.lr,
            eval_every=max(1, args.steps // 4), device=device, seed=args.seed,
        )
        result["gen_bytes_per_second"] = round(generation_speed(model, device), 1)
        if getattr(model, "last_generate_stats", None):
            result["bytes_per_pass"] = model.last_generate_stats["bytes_per_pass"]
        if hasattr(model, "iterations"):  # the looped members: quality as the loop count changes
            result["bpb_by_iterations"] = {}
            for r in (1, 2, 4, 8, 16):
                model.iterations = r
                result["bpb_by_iterations"][r] = round(evaluate(model, data, 20, args.batch_size, device), 4)
            model.iterations = None
        results.append(result)

    out = LAB / "runs" / "bench"
    out.mkdir(parents=True, exist_ok=True)
    (out / "results.json").write_text(json.dumps({"config": cfg.to_dict(), "steps": args.steps, "results": results}, indent=2))

    print(f"\n{args.steps} steps, batch {args.batch_size}, context {cfg.seq_len}, width {cfg.d_model}, {cfg.n_layers} layers, {device}\n")
    print("| model | params | val bits/byte | train bytes/s | generate bytes/s |")
    print("|---|---:|---:|---:|---:|")
    for r in results:
        bound = " (bound)" if not FAMILY[r["model"]].autoregressive else ""
        print(
            f"| {r['model']} | {r['params'] / 1e6:.2f}M | {r['val_bpb']:.3f}{bound} "
            f"| {r['bytes_per_second']:,} | {r['gen_bytes_per_second']:,} |"
        )
    for r in results:
        if "bpb_by_iterations" in r:
            curve = ", ".join(f"r={k}: {v:.3f}" for k, v in r["bpb_by_iterations"].items())
            print(f"\n{r['model']}, bits/byte by loop count: {curve}")
        if "bytes_per_pass" in r:
            print(f"\n{r['model']}, bytes written per backbone pass: {r['bytes_per_pass']}")
    print(f"\n-> {out / 'results.json'}")


if __name__ == "__main__":
    main()
