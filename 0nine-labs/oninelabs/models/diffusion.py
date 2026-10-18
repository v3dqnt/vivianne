"""Viv-D: masked discrete diffusion. Not autoregressive: text is written all at once and refined.

Training hides a random fraction t of the bytes behind a [MASK] symbol and a bidirectional
transformer fills them in. Generation starts from a block of all [MASK] and reveals the bytes
it is most confident about first, over a fixed number of steps, so a block of N bytes takes
`sample_steps` forward passes instead of N.

The loss is the continuous-time ELBO from MDLM (Sahoo et al. 2024) with a linear schedule:
E_t[ (1/t) * CE over the masked bytes ] / T. It is an upper bound on the true NLL, so its
bits-per-byte is directly comparable to (and slightly pessimistic against) the AR members.
"""

import math

import torch
import torch.nn as nn
import torch.nn.functional as F

from .common import VOCAB, ByteModel, Config, TransformerBlock

MASK = VOCAB  # one extra input symbol; the model never outputs it


class Diffusion(ByteModel):
    name = "diffusion"
    autoregressive = False

    def __init__(self, cfg: Config):
        super().__init__(cfg, vocab=VOCAB + 1)
        self.head = nn.Linear(cfg.d_model, VOCAB, bias=False)
        self.blocks = nn.ModuleList(TransformerBlock(cfg, causal=False) for _ in range(cfg.n_layers))
        self.init_weights()

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        h = self.embed(x)
        for block in self.blocks:
            h = block(h)
        return self.head(self.norm(h))

    def loss(self, x: torch.Tensor) -> torch.Tensor:
        x = x[:, :-1]
        b, t = x.shape
        # low-discrepancy t: one draw per sequence, spread evenly over (0, 1]
        level = ((torch.rand(1, device=x.device) + torch.arange(b, device=x.device) / b) % 1).clamp_min(1e-3)
        masked = torch.rand(b, t, device=x.device) < level[:, None]
        logits = self(torch.where(masked, MASK, x))
        ce = F.cross_entropy(logits.reshape(-1, VOCAB), x.reshape(-1), reduction="none").view(b, t)
        return ((ce * masked).sum(1) / level).mean() / t

    @torch.no_grad()
    def generate(self, prompt: torch.Tensor, n: int, temperature: float = 0.8, top_k: int = 40) -> torch.Tensor:
        """Writes n bytes in blocks of up to half the context, each block denoised in parallel."""
        x = prompt
        block = max(1, self.cfg.seq_len // 2)
        while n > 0:
            size = min(n, block)
            x = torch.cat([x, x.new_full((x.size(0), size), MASK)], dim=1)
            x = self._denoise(x, size, temperature, top_k)
            n -= size
        return x

    def _denoise(self, x: torch.Tensor, size: int, temperature: float, top_k: int) -> torch.Tensor:
        window = x[:, -self.cfg.seq_len :].clone()
        steps = min(self.cfg.sample_steps, size)
        for k in range(steps):
            masked = window == MASK
            reveal = math.ceil(int(masked[0].sum()) / (steps - k))
            logits = self(window) / max(temperature, 1e-5)
            if top_k:
                kth = torch.topk(logits, min(top_k, VOCAB)).values[..., -1:]
                logits = logits.masked_fill(logits < kth, float("-inf"))
            probs = F.softmax(logits, -1)
            choice = torch.multinomial(probs.view(-1, VOCAB), 1).view(window.shape)
            confidence = probs.gather(-1, choice.unsqueeze(-1)).squeeze(-1).masked_fill(~masked, -1.0)
            pick = torch.topk(confidence, reveal, dim=1).indices
            window.scatter_(1, pick, choice.gather(1, pick))
        return torch.cat([x[:, : -window.size(1)], window], dim=1) if x.size(1) > window.size(1) else window
