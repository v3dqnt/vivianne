"""Viv-H: the family's good traits in one model.

    bytes -> SSM prelude -> [SSM ... SSM, attention] x r -> SSM coda -> next byte      (the backbone)
                                                       \\-> diffusion drafter -> next block

- From Viv-S: the backbone is mostly selective state-space layers (linear in length, best per parameter).
- From Viv-T: one attention layer, for exact lookups an SSM's fixed-size memory can blur.
- From Viv-L: the core is shared and looped r times, and r can be raised at inference.
- From Viv-D: a small bidirectional drafter proposes the next `block_size` bytes in `draft_steps`
  denoising steps, conditioned on the backbone's state. The backbone checks the whole block in
  one pass and keeps the longest prefix that matches what it samples itself, plus one byte of
  its own. Each kept byte is a byte the backbone sampled, so the output follows the backbone's
  own distribution exactly; the drafter only decides how many bytes each backbone pass yields.

Training loss is the backbone's next-byte loss plus `draft_weight` times the drafter's masked
diffusion loss (MDLM ELBO, one masking level per block). In eval mode `loss` is the backbone's
loss alone, so its bits per byte are exact and comparable with the other members.
"""

import math

import torch
import torch.nn as nn
import torch.nn.functional as F

from .common import VOCAB, ByteModel, Config, RMSNorm, TransformerBlock, sample
from .looped import sample_iterations
from .ssm import SelectiveSSMBlock

MASK = VOCAB


class Hybrid(ByteModel):
    name = "hybrid"

    def __init__(self, cfg: Config):
        super().__init__(cfg)
        assert cfg.n_layers >= 3, "hybrid needs a prelude, at least one core layer, and a coda"
        d = cfg.d_model
        self.prelude = SelectiveSSMBlock(cfg)
        self.inject = nn.Linear(2 * d, d, bias=False)
        self.core = nn.ModuleList(
            [SelectiveSSMBlock(cfg) for _ in range(cfg.n_layers - 3)] + [TransformerBlock(cfg, causal=True)]
        )
        self.core_norm = RMSNorm(d)
        self.coda = SelectiveSSMBlock(cfg)

        self.mask_embed = nn.Parameter(torch.zeros(d))
        self.ctx_proj = nn.Linear(d, d, bias=False)
        self.drafter = nn.ModuleList(TransformerBlock(cfg, causal=False) for _ in range(cfg.draft_layers))
        self.draft_norm = RMSNorm(d)
        self.draft_head = nn.Linear(d, VOCAB, bias=False)

        self.init_weights()
        for m in self.modules():
            if isinstance(m, SelectiveSSMBlock):
                m.reset_dt()
        self.iterations: int | None = None  # set to fix r at inference; None samples it in training
        self.last_generate_stats: dict = {}

    # --- backbone ---

    def hidden(self, x: torch.Tensor, iterations: int | None = None) -> torch.Tensor:
        cfg = self.cfg
        r = iterations or self.iterations or (sample_iterations(cfg.core_iterations) if self.training else cfg.core_iterations)
        e = self.prelude(self.embed(x))
        s = torch.zeros_like(e)
        no_grad_steps = max(0, r - cfg.backprop_iterations) if self.training else 0
        for i in range(r):
            with torch.set_grad_enabled(torch.is_grad_enabled() and i >= no_grad_steps):
                s = self.inject(torch.cat([s, e], dim=-1))
                for block in self.core:
                    s = block(s)
                s = self.core_norm(s)
        return self.norm(self.coda(s))

    def forward(self, x: torch.Tensor, iterations: int | None = None) -> torch.Tensor:
        return self.head(self.hidden(x, iterations))

    # --- drafter ---

    def draft_logits(self, ctx: torch.Tensor, lead: torch.Tensor, block: torch.Tensor) -> torch.Tensor:
        """ctx: backbone state two bytes before the block (n, d); lead: the byte just before it (n,);
        block: (n, k) bytes with MASK where unknown. Returns logits (n, k, 256)."""
        masked = (block == MASK).unsqueeze(-1)
        tokens = torch.where(masked, self.mask_embed, self.embed(block.clamp(max=VOCAB - 1)))
        h = torch.cat([self.ctx_proj(ctx).unsqueeze(1), self.embed(lead).unsqueeze(1), tokens], dim=1)
        for layer in self.drafter:
            h = layer(h)
        return self.draft_head(self.draft_norm(h[:, 2:]))

    def draft(self, ctx: torch.Tensor, lead: torch.Tensor, k: int, temperature: float, top_k: int) -> torch.Tensor:
        block = lead.new_full((lead.size(0), k), MASK)
        steps = min(self.cfg.draft_steps, k)
        for step in range(steps):
            masked = block == MASK
            reveal = math.ceil(int(masked[0].sum()) / (steps - step))
            logits = self.draft_logits(ctx, lead, block)
            choice = sample(logits, temperature, top_k)
            confidence = F.softmax(logits, -1).gather(-1, choice.unsqueeze(-1)).squeeze(-1).masked_fill(~masked, -1.0)
            pick = torch.topk(confidence, reveal, dim=1).indices
            block.scatter_(1, pick, choice.gather(1, pick))
        return block

    # --- training ---

    def loss(self, x: torch.Tensor) -> torch.Tensor:
        inp, target = x[:, :-1], x[:, 1:]
        h = self.hidden(inp)
        ar = F.cross_entropy(self.head(h).reshape(-1, VOCAB), target.reshape(-1))
        if not self.training:
            return ar

        k, (b, t) = self.cfg.block_size, inp.shape
        starts = torch.arange(2, t - k + 1, k, device=x.device)
        if len(starts) == 0:
            return ar
        blocks = inp[:, starts[:, None] + torch.arange(k, device=x.device)].reshape(-1, k)  # (b * nb, k)
        ctx = h[:, starts - 2].reshape(-1, h.size(-1))
        lead = inp[:, starts - 1].reshape(-1)
        n = blocks.size(0)
        level = ((torch.rand(1, device=x.device) + torch.arange(n, device=x.device) / n) % 1).clamp_min(1e-3)
        masked = torch.rand(n, k, device=x.device) < level[:, None]
        logits = self.draft_logits(ctx, lead, torch.where(masked, MASK, blocks))
        ce = F.cross_entropy(logits.reshape(-1, VOCAB), blocks.reshape(-1), reduction="none").view(n, k)
        diffusion = ((ce * masked).sum(1) / level).mean() / k
        return ar + self.cfg.draft_weight * diffusion

    # --- generation: draft a block, verify it in one backbone pass ---

    @torch.no_grad()
    def generate(self, prompt: torch.Tensor, n: int, temperature: float = 0.8, top_k: int = 40) -> torch.Tensor:
        if prompt.size(0) != 1:  # drafts are accepted per sequence, so a batch decodes plainly
            return super().generate(prompt, n, temperature, top_k)
        if prompt.size(1) < 2 and n > 0:  # the drafter needs two bytes of context
            return self.generate(super().generate(prompt, 1, temperature, top_k), n - 1, temperature, top_k)

        window, x = self.cfg.seq_len, prompt
        ctx = self.hidden(x[:, -window:])[:, -2]
        passes, made = 1, 0
        while made < n:
            k = min(self.cfg.block_size, n - made)
            proposal = self.draft(ctx, x[:, -1], k, temperature, top_k)
            h = self.hidden(torch.cat([x, proposal], dim=1)[:, -window:])
            passes += 1
            own = sample(self.head(h[:, -k - 1 :]), temperature, top_k)  # the backbone's pick at each spot
            agree = int((own[0, :k] == proposal[0]).long().cumprod(0).sum())
            x = torch.cat([x, proposal[:, :agree], own[:, agree : agree + 1]], dim=1)
            made += agree + 1
            ctx = h[:, agree - k - 1]  # backbone state at the last byte both agreed on
        self.last_generate_stats = {"bytes_per_pass": round(made / passes, 2), "passes": passes}
        return x[:, : prompt.size(1) + n]
