"""Viv-L: recurrent depth. One shared core block is applied over and over in latent space.

prelude -> [core x r] -> coda. The number of iterations r is random during training, so at
inference you choose it: more iterations is more thinking per byte, with no extra parameters
and no extra tokens. The research question is how far test-time compute in latent space can
substitute for parameters (in the spirit of Geiping et al. 2025, "Scaling up Test-Time Compute
with Latent Reasoning").

n_layers is split as 1 prelude layer, n_layers - 2 core layers, 1 coda layer.
"""

import torch
import torch.nn as nn

from .common import ByteModel, Config, RMSNorm, TransformerBlock


def sample_iterations(mean: int) -> int:
    """A loop count for one training step: log-normal around `mean`, with a tail long enough that
    the model also sees deep runs."""
    r = torch.distributions.LogNormal(torch.log(torch.tensor(float(mean))) - 0.5, 1.0).sample()
    return int(r.clamp(1, 4 * mean).round())


class Looped(ByteModel):
    name = "looped"

    def __init__(self, cfg: Config):
        super().__init__(cfg)
        assert cfg.n_layers >= 3, "looped needs a prelude, at least one core layer, and a coda"
        self.prelude = TransformerBlock(cfg)
        self.inject = nn.Linear(2 * cfg.d_model, cfg.d_model, bias=False)
        self.core = nn.ModuleList(TransformerBlock(cfg) for _ in range(cfg.n_layers - 2))
        self.core_norm = RMSNorm(cfg.d_model)
        self.coda = TransformerBlock(cfg)
        self.init_weights()
        self.iterations: int | None = None  # set to fix r at inference; None samples it in training

    def forward(self, x: torch.Tensor, iterations: int | None = None) -> torch.Tensor:
        r = iterations or self.iterations or (sample_iterations(self.cfg.mean_iterations) if self.training else self.cfg.mean_iterations)
        e = self.prelude(self.embed(x))
        s = torch.randn_like(e) * 0.02
        # truncated backprop: only the last few iterations get gradients, so memory is flat in r
        no_grad_steps = max(0, r - self.cfg.backprop_iterations) if self.training else 0
        for i in range(r):
            with torch.set_grad_enabled(torch.is_grad_enabled() and i >= no_grad_steps):
                s = self.inject(torch.cat([s, e], dim=-1))
                for block in self.core:
                    s = block(s)
                s = self.core_norm(s)
        return self.head(self.norm(self.coda(s)))
