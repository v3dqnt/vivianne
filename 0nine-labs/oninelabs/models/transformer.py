"""Viv-T: the reference. A plain causal transformer (pre-norm, RoPE, SwiGLU).

Not the point of the lab, but every other member is measured against it, so it stays boring on purpose.
"""

import torch
import torch.nn as nn

from .common import ByteModel, Config, TransformerBlock


class Transformer(ByteModel):
    name = "transformer"

    def __init__(self, cfg: Config):
        super().__init__(cfg)
        self.blocks = nn.ModuleList(TransformerBlock(cfg, causal=True) for _ in range(cfg.n_layers))
        self.init_weights()

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        h = self.embed(x)
        for block in self.blocks:
            h = block(h)
        return self.head(self.norm(h))
