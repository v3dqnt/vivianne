"""Pieces every model in the family shares: the config, norms, MLPs, attention, and the base class."""

from __future__ import annotations

import math
from dataclasses import asdict, dataclass

import torch
import torch.nn as nn
import torch.nn.functional as F

VOCAB = 256  # every model reads and writes raw bytes; no tokenizer


@dataclass
class Config:
    d_model: int = 256
    n_layers: int = 4
    n_heads: int = 4
    seq_len: int = 256
    dropout: float = 0.0
    # ssm
    d_state: int = 16
    expand: int = 2
    conv_kernel: int = 4
    # looped: prelude and coda layers, and how many times the core runs
    mean_iterations: int = 4
    backprop_iterations: int = 4
    # diffusion
    sample_steps: int = 64

    def to_dict(self) -> dict:
        return asdict(self)


class RMSNorm(nn.Module):
    def __init__(self, d: int, eps: float = 1e-6):
        super().__init__()
        self.eps = eps
        self.weight = nn.Parameter(torch.ones(d))

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return x * torch.rsqrt(x.pow(2).mean(-1, keepdim=True) + self.eps) * self.weight


class SwiGLU(nn.Module):
    def __init__(self, d: int, dropout: float = 0.0):
        super().__init__()
        hidden = 64 * math.ceil(8 * d / 3 / 64)
        self.gate_up = nn.Linear(d, 2 * hidden, bias=False)
        self.down = nn.Linear(hidden, d, bias=False)
        self.drop = nn.Dropout(dropout)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        gate, up = self.gate_up(x).chunk(2, dim=-1)
        return self.drop(self.down(F.silu(gate) * up))


def rotary(seq_len: int, head_dim: int, device: torch.device) -> tuple[torch.Tensor, torch.Tensor]:
    inv_freq = 1.0 / (10000 ** (torch.arange(0, head_dim, 2, device=device).float() / head_dim))
    freqs = torch.outer(torch.arange(seq_len, device=device).float(), inv_freq)
    return freqs.cos(), freqs.sin()


def apply_rotary(x: torch.Tensor, cos: torch.Tensor, sin: torch.Tensor) -> torch.Tensor:
    x1, x2 = x[..., ::2], x[..., 1::2]
    out = torch.stack((x1 * cos - x2 * sin, x1 * sin + x2 * cos), dim=-1)
    return out.flatten(-2).type_as(x)


class Attention(nn.Module):
    def __init__(self, cfg: Config, causal: bool):
        super().__init__()
        assert cfg.d_model % cfg.n_heads == 0
        self.n_heads = cfg.n_heads
        self.causal = causal
        self.qkv = nn.Linear(cfg.d_model, 3 * cfg.d_model, bias=False)
        self.out = nn.Linear(cfg.d_model, cfg.d_model, bias=False)
        self.dropout = cfg.dropout

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        b, t, d = x.shape
        q, k, v = self.qkv(x).view(b, t, 3, self.n_heads, d // self.n_heads).permute(2, 0, 3, 1, 4)
        cos, sin = rotary(t, d // self.n_heads, x.device)
        q, k = apply_rotary(q, cos, sin), apply_rotary(k, cos, sin)
        y = F.scaled_dot_product_attention(
            q, k, v, is_causal=self.causal, dropout_p=self.dropout if self.training else 0.0
        )
        return self.out(y.transpose(1, 2).reshape(b, t, d))


class TransformerBlock(nn.Module):
    def __init__(self, cfg: Config, causal: bool = True):
        super().__init__()
        self.norm1 = RMSNorm(cfg.d_model)
        self.attn = Attention(cfg, causal)
        self.norm2 = RMSNorm(cfg.d_model)
        self.mlp = SwiGLU(cfg.d_model, cfg.dropout)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = x + self.attn(self.norm1(x))
        return x + self.mlp(self.norm2(x))


class ByteModel(nn.Module):
    """Base for the family. Subclasses implement `forward(x) -> logits` over the next byte.

    Every model exposes the same three things so the trainer and benchmark treat them alike:
    `loss(x)` in nats per byte, `generate(prompt, n)`, and `num_params()`.
    Non-autoregressive members override `loss` and `generate`.
    """

    name = "base"
    autoregressive = True

    def __init__(self, cfg: Config, vocab: int = VOCAB):
        super().__init__()
        self.cfg = cfg
        self.vocab = vocab
        self.embed = nn.Embedding(vocab, cfg.d_model)
        self.norm = RMSNorm(cfg.d_model)
        self.head = nn.Linear(cfg.d_model, vocab, bias=False)

    def init_weights(self) -> None:
        for m in self.modules():
            if isinstance(m, nn.Linear):
                nn.init.normal_(m.weight, std=0.02)
                if m.bias is not None:
                    nn.init.zeros_(m.bias)
            elif isinstance(m, nn.Embedding):
                nn.init.normal_(m.weight, std=0.02)

    def num_params(self) -> int:
        return sum(p.numel() for p in self.parameters())

    def loss(self, x: torch.Tensor) -> torch.Tensor:
        logits = self(x[:, :-1])
        return F.cross_entropy(logits.reshape(-1, logits.size(-1)), x[:, 1:].reshape(-1))

    @torch.no_grad()
    def generate(self, prompt: torch.Tensor, n: int, temperature: float = 0.8, top_k: int = 40) -> torch.Tensor:
        x = prompt
        for _ in range(n):
            logits = self(x[:, -self.cfg.seq_len :])[:, -1] / max(temperature, 1e-5)
            if top_k:
                kth = torch.topk(logits, min(top_k, logits.size(-1))).values[:, -1:]
                logits = logits.masked_fill(logits < kth, float("-inf"))
            x = torch.cat([x, torch.multinomial(F.softmax(logits, -1), 1)], dim=1)
        return x
