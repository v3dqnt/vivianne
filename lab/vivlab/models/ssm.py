"""Viv-S: a selective state-space model (Mamba-style), no attention anywhere.

Each layer keeps a fixed-size state per channel and decides, per byte, how much of it to keep
(dt), what to write (B) and what to read (C). Cost is linear in sequence length and the state
needed to generate is constant, which is what makes this family interesting for small hardware.

The scan here is a readable sequential loop. A fused or chunked parallel scan is an open
hardware-efficiency task for the lab (see lab/README.md).
"""

import math

import torch
import torch.nn as nn
import torch.nn.functional as F

from .common import ByteModel, Config, RMSNorm


def selective_scan(u, dt, A, B, C, D):
    """h_t = exp(dt_t * A) * h_{t-1} + dt_t * B_t * u_t ;  y_t = <h_t, C_t> + D * u_t

    u, dt: (b, t, d)   A: (d, n)   B, C: (b, t, n)   D: (d,)
    """
    b, t, d = u.shape
    decay = torch.exp(dt.unsqueeze(-1) * A)  # (b, t, d, n)
    write = (dt * u).unsqueeze(-1) * B.unsqueeze(2)  # (b, t, d, n)
    h = u.new_zeros(b, d, A.size(1))
    ys = []
    for i in range(t):
        h = decay[:, i] * h + write[:, i]
        ys.append(torch.einsum("bdn,bn->bd", h, C[:, i]))
    return torch.stack(ys, dim=1) + u * D


class SelectiveSSMBlock(nn.Module):
    def __init__(self, cfg: Config):
        super().__init__()
        d_inner = cfg.expand * cfg.d_model
        self.d_state = cfg.d_state
        self.dt_rank = math.ceil(cfg.d_model / 16)
        self.norm = RMSNorm(cfg.d_model)
        self.in_proj = nn.Linear(cfg.d_model, 2 * d_inner, bias=False)
        # depthwise conv, padded on the left only so position t never sees t+1
        self.conv = nn.Conv1d(d_inner, d_inner, cfg.conv_kernel, groups=d_inner, padding=cfg.conv_kernel - 1)
        self.x_proj = nn.Linear(d_inner, self.dt_rank + 2 * cfg.d_state, bias=False)
        self.dt_proj = nn.Linear(self.dt_rank, d_inner, bias=True)
        self.A_log = nn.Parameter(torch.log(torch.arange(1, cfg.d_state + 1).float()).repeat(d_inner, 1))
        self.D = nn.Parameter(torch.ones(d_inner))
        self.out_proj = nn.Linear(d_inner, cfg.d_model, bias=False)
        self.drop = nn.Dropout(cfg.dropout)

    def reset_dt(self) -> None:
        # dt starts between 1e-3 and 1e-1 so early layers neither forget everything nor nothing
        dt = torch.exp(torch.rand(self.dt_proj.bias.shape) * (math.log(0.1) - math.log(1e-3)) + math.log(1e-3))
        with torch.no_grad():
            self.dt_proj.bias.copy_(dt + torch.log(-torch.expm1(-dt)))  # inverse softplus

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        t = x.size(1)
        u, z = self.in_proj(self.norm(x)).chunk(2, dim=-1)
        u = F.silu(self.conv(u.transpose(1, 2))[..., :t].transpose(1, 2))
        dt_in, B, C = self.x_proj(u).split([self.dt_rank, self.d_state, self.d_state], dim=-1)
        dt = F.softplus(self.dt_proj(dt_in))
        y = selective_scan(u, dt, -torch.exp(self.A_log), B, C, self.D)
        return x + self.drop(self.out_proj(y * F.silu(z)))


class SSM(ByteModel):
    name = "ssm"

    def __init__(self, cfg: Config):
        super().__init__(cfg)
        self.blocks = nn.ModuleList(SelectiveSSMBlock(cfg) for _ in range(cfg.n_layers))
        self.init_weights()
        for block in self.blocks:
            block.reset_dt()

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        h = self.embed(x)
        for block in self.blocks:
            h = block(h)
        return self.head(self.norm(h))
