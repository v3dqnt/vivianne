"""Train one member of the family.

    python -m vivlab.train --model ssm --steps 2000
    python -m vivlab.train --model looped --data shakespeare --d-model 384 --n-layers 6

Writes runs/<name>/{config.json, log.jsonl, model.pt} and prints a sample at the end.
"""

from __future__ import annotations

import argparse
import json
import math
import time
from dataclasses import fields
from pathlib import Path

import torch

from .data import LAB, ByteData
from .models import FAMILY, ByteModel, Config, build

LN2 = math.log(2)


def pick_device(name: str = "auto") -> torch.device:
    if name != "auto":
        return torch.device(name)
    if torch.cuda.is_available():
        return torch.device("cuda")
    if torch.backends.mps.is_available():
        return torch.device("mps")
    return torch.device("cpu")


@torch.no_grad()
def evaluate(model: ByteModel, data: ByteData, batches: int, batch_size: int, device: torch.device) -> float:
    """Validation loss in bits per byte. For diffusion it is the ELBO bound, so an upper bound."""
    model.eval()
    rng = torch.random.get_rng_state()
    torch.manual_seed(1234)  # the same windows every time, so numbers across runs line up
    total = 0.0
    for _ in range(batches):
        x = data.batch("val", batch_size, model.cfg.seq_len, device)
        total += model.loss(x).item()
    torch.random.set_rng_state(rng)
    model.train()
    return total / batches / LN2


def train(
    name: str,
    cfg: Config,
    data: ByteData,
    steps: int = 1000,
    batch_size: int = 16,
    lr: float = 1e-3,
    warmup: int = 100,
    eval_every: int = 250,
    eval_batches: int = 20,
    device: torch.device | None = None,
    seed: int = 0,
    out: Path | None = None,
    log=print,
) -> tuple[ByteModel, dict]:
    device = device or pick_device()
    torch.manual_seed(seed)
    model = build(name, cfg).to(device)
    decay = [p for p in model.parameters() if p.dim() >= 2]
    no_decay = [p for p in model.parameters() if p.dim() < 2]
    opt = torch.optim.AdamW(
        [{"params": decay, "weight_decay": 0.1}, {"params": no_decay, "weight_decay": 0.0}], lr=lr, betas=(0.9, 0.95)
    )

    def lr_at(step: int) -> float:
        if step < warmup:
            return lr * (step + 1) / warmup
        progress = (step - warmup) / max(1, steps - warmup)
        return lr * (0.1 + 0.9 * 0.5 * (1 + math.cos(math.pi * progress)))

    if out:
        out.mkdir(parents=True, exist_ok=True)
        (out / "config.json").write_text(json.dumps({"model": name, **cfg.to_dict()}, indent=2))
        logfile = (out / "log.jsonl").open("w")
    log(f"{name}: {model.num_params() / 1e6:.2f}M params on {device}, {data}")

    history = []
    tokens, start = 0, time.perf_counter()
    for step in range(steps + 1):
        if step % eval_every == 0 or step == steps:
            val = evaluate(model, data, eval_batches, batch_size, device)
            elapsed = time.perf_counter() - start
            row = {"step": step, "val_bpb": round(val, 4), "tokens": tokens, "seconds": round(elapsed, 1)}
            history.append(row)
            log(f"  step {step:>6}  val {val:.3f} bpb  {tokens / max(elapsed, 1e-9):>9,.0f} bytes/s")
            if out:
                logfile.write(json.dumps(row) + "\n")
                logfile.flush()
        if step == steps:
            break
        for group in opt.param_groups:
            group["lr"] = lr_at(step)
        x = data.batch("train", batch_size, cfg.seq_len, device)
        loss = model.loss(x)
        opt.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        tokens += x.numel()

    seconds = time.perf_counter() - start
    result = {
        "model": name,
        "params": model.num_params(),
        "val_bpb": history[-1]["val_bpb"],
        "seconds": round(seconds, 1),
        "bytes_per_second": round(tokens / seconds),
        "history": history,
    }
    if out:
        logfile.close()
        torch.save({"model": name, "config": cfg.to_dict(), "state": model.state_dict()}, out / "model.pt")
    return model, result


def sample(model: ByteModel, prompt: str, n: int, device: torch.device) -> str:
    model.eval()
    x = torch.tensor([list(prompt.encode())], device=device)
    return bytes(model.generate(x, n)[0].tolist()).decode(errors="replace")


def load(path: Path, device: torch.device | str = "cpu") -> ByteModel:
    ckpt = torch.load(path, map_location=device)
    model = build(ckpt["model"], Config(**ckpt["config"])).to(device)
    model.load_state_dict(ckpt["state"])
    return model.eval()


def config_args(parser: argparse.ArgumentParser) -> None:
    for f in fields(Config):
        parser.add_argument(f"--{f.name.replace('_', '-')}", type=type(f.default), default=f.default)


def config_from(args: argparse.Namespace) -> Config:
    return Config(**{f.name: getattr(args, f.name) for f in fields(Config)})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--model", choices=FAMILY, required=True)
    parser.add_argument("--data", default=None, help="file, directory, or 'shakespeare' (default: this repo's docs)")
    parser.add_argument("--steps", type=int, default=1000)
    parser.add_argument("--batch-size", type=int, default=16)
    parser.add_argument("--lr", type=float, default=1e-3)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--run", default=None, help="run name (default: the model name)")
    parser.add_argument("--prompt", default="Vivianne is")
    config_args(parser)
    args = parser.parse_args()

    device = pick_device(args.device)
    out = LAB / "runs" / (args.run or args.model)
    model, result = train(
        args.model, config_from(args), ByteData(args.data), args.steps, args.batch_size, args.lr,
        device=device, seed=args.seed, out=out,
    )
    print(f"\nfinal: {result['val_bpb']:.3f} bpb in {result['seconds']}s -> {out}")
    print("\n--- sample ---\n" + sample(model, args.prompt, 300, device))


if __name__ == "__main__":
    main()
