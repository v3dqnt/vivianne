# 0nine Labs

**0nine Labs** (written 09 Labs, said "o-nine labs") is a small research lab and community working on the edge of AI: new kinds of models beyond the usual LLM, hardware that runs them efficiently, and devices that work together seamlessly whatever the brand.

The lab's first project is [Vivianne](../README.md), the harness: it runs any model and does real work with it. This folder is where we build our own models, and the bet is that the next good small model won't necessarily be a bigger transformer.

So the lab starts as a **family of models with different architectures**, kept small enough to train on a laptop and set up so they can be compared fairly. Every member:

- reads and writes **raw bytes**, so there's no tokenizer and any file is a dataset,
- has the same interface (`loss`, `generate`, `num_params`), so one trainer and one benchmark serve all of them,
- has a test proving it learns, and the autoregressive members have a test proving they never see the future.

## The family

| Member | Code | Idea | What we want to learn |
|---|---|---|---|
| **Viv-T** | `transformer` | Causal transformer (RoPE, SwiGLU, RMSNorm). The reference. | The baseline every other member has to beat at equal parameters and compute. |
| **Viv-S** | `ssm` | Selective state space (Mamba-style), no attention. Each layer has a fixed-size memory and learns per byte what to keep, write and read. | Linear cost in length and constant memory while generating. How close can it get to attention on recall-heavy text? |
| **Viv-L** | `looped` | Recurrent depth: one shared core block run *r* times in latent space, with *r* picked at inference. | Can it "think longer" without more parameters or tokens? Does quality keep improving as *r* grows past what it was trained on? |
| **Viv-D** | `diffusion` | Masked discrete diffusion. Starts from a block of blanks and fills in the bytes it is surest of first. | Writes N bytes in far fewer than N forward passes. What do we lose in quality, and what do we gain in speed and editing (infilling)? |

Next candidates, roughly in order: a hybrid (SSM layers with a few attention layers), a mixture-of-experts version of the winner, an energy-based or JEPA-style model that predicts in latent space instead of bytes, and liquid / continuous-time networks for sensor and device signals.

## Run it

Needs Python 3.10+ and PyTorch. A GPU (CUDA or Apple MPS) is used automatically; CPU works for the small configs.

```bash
cd 0nine-labs
python -m venv .venv && source .venv/bin/activate     # Windows: .venv\Scripts\activate
pip install torch pytest

pytest                                               # every member learns; AR members are causal
python -m oninelabs.train --model ssm --steps 2000      # train one, print a sample
python -m oninelabs.bench --steps 1000                  # train all four the same way, compare
```

With no `--data`, training uses this repository's own Markdown (about 500 KB), so nothing has to be downloaded. `--data shakespeare` fetches Tiny Shakespeare, and `--data path/to/file-or-folder` uses your own text. Model size comes from flags like `--d-model 384 --n-layers 6 --seq-len 512`. Every run goes to `runs/<name>/` (config, log, weights).

Results are reported in **bits per byte**, which is comparable across all members because none of them use a tokenizer. Viv-D's number is an upper bound (it's an ELBO), so it's a slightly pessimistic comparison for diffusion.

## Layout

```
oninelabs/
  models/common.py       shared config, norms, attention, the ByteModel base class
  models/transformer.py  Viv-T
  models/ssm.py          Viv-S
  models/looped.py       Viv-L
  models/diffusion.py    Viv-D
  data.py                byte datasets
  train.py               the trainer (python -m oninelabs.train)
  bench.py               the comparison (python -m oninelabs.bench)
tests/                   pytest
```

Adding a member takes one file in `models/` that subclasses `ByteModel`, one line in `models/__init__.py`, and the shared tests pick it up.

## Research agenda

**1. Architectures.** The family above. Compare members at matched parameters *and* matched compute, at 3 or more sizes, so we see trends and not single points. A result only counts once it holds across seeds.

**2. Efficiency on real hardware.** Small models are only useful if they're fast on the machines people own. Open tasks:
- Viv-S's scan is a readable Python loop. Write a chunked parallel scan, then a fused kernel (Triton on CUDA, Metal on Apple), and measure the speedup.
- Quantize the winners to 8 and 4 bits and measure what breaks.
- Give Viv-S a recurrent generation path (one state update per byte instead of re-reading the context). That's where its constant memory turns into real speed.
- Track bytes per second and bytes per joule on a laptop CPU, an integrated GPU, and a phone.

**3. Into Vivianne.** Export the best member to a format Vivianne's local runtime can load (GGUF via llama.cpp, or ONNX), so the lab's models show up in Settings → Providers next to everything else.

**4. Devices that work together.** Later: small models that run on the devices themselves (phones, laptops, home hardware) and coordinate over a shared, brand-neutral protocol, with Vivianne as the hub. The constant-memory members (Viv-S) and tiny signal models are the natural fit.

## How we work

- Every claim comes with the command that reproduces it and the `runs/` log it came from.
- Negative results get written down too. "Viv-D is worse at this size" is a finding.
- Keep members small and readable. A new idea should fit in one file you can read in ten minutes.
