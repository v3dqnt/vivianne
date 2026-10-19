import math

import pytest
import torch

from oninelabs import FAMILY, Config, build
from oninelabs.data import ByteData

TINY = Config(d_model=64, n_layers=3, n_heads=4, seq_len=32, d_state=8, mean_iterations=2, sample_steps=8)


@pytest.fixture(params=list(FAMILY))
def model(request):
    torch.manual_seed(0)
    return build(request.param, TINY)


def batch(b=4, t=TINY.seq_len + 1):
    return torch.randint(0, 256, (b, t))


def test_loss_starts_near_uniform(model):
    # a fresh model should be close to guessing among 256 bytes, ~ln(256) nats
    loss = model.eval().loss(batch()).item()
    assert math.isfinite(loss)
    assert abs(loss - math.log(256)) < 1.0


def seeded_loss(model, x):
    # diffusion's loss depends on which bytes it hides, so fix that draw to compare before/after
    torch.manual_seed(1)
    with torch.no_grad():
        return model.loss(x).item()


def test_learns_a_fixed_batch(model):
    text = torch.tensor(list(b"the quick brown fox jumps over the lazy dog. " * 8))
    x = torch.stack([text[i : i + TINY.seq_len + 1] for i in (0, 7, 19, 30)])
    opt = torch.optim.AdamW(model.parameters(), lr=3e-3)
    first = seeded_loss(model, x)
    for _ in range(150):
        loss = model.loss(x)
        opt.zero_grad()
        loss.backward()
        opt.step()
    assert seeded_loss(model, x) < 0.6 * first


def test_generate_appends_bytes(model):
    model.eval()
    out = model.generate(torch.tensor([list(b"hello")]), 20)
    assert out.shape == (1, 25)
    assert out[0, :5].tolist() == list(b"hello")
    assert int(out.max()) < 256 and int(out.min()) >= 0


@pytest.mark.parametrize("name", [n for n, cls in FAMILY.items() if cls.autoregressive])
def test_autoregressive_members_are_causal(name):
    # changing a later byte must never change the prediction for an earlier position
    torch.manual_seed(0)
    model = build(name, TINY).eval()
    if hasattr(model, "iterations"):
        model.iterations = 2
    x = batch(1, TINY.seq_len)
    y = x.clone()
    y[0, 20:] = (y[0, 20:] + 1) % 256
    torch.manual_seed(1)
    a = model(x)
    torch.manual_seed(1)
    b = model(y)
    assert torch.allclose(a[0, :20], b[0, :20], atol=1e-5)
    assert not torch.allclose(a[0, 20:], b[0, 20:], atol=1e-5)


def test_looped_runs_any_number_of_iterations():
    model = build("looped", TINY).eval()
    x = batch(1, TINY.seq_len)
    for r in (1, 3, 10):
        assert model(x, iterations=r).shape == (1, TINY.seq_len, 256)


def test_diffusion_is_bidirectional():
    torch.manual_seed(0)
    model = build("diffusion", TINY).eval()
    x = batch(1, TINY.seq_len)
    y = x.clone()
    y[0, -1] = (y[0, -1] + 1) % 256
    assert not torch.allclose(model(x)[0, 0], model(y)[0, 0])


def test_data_windows(tmp_path):
    (tmp_path / "a.txt").write_bytes(bytes(range(256)) * 20)
    data = ByteData(str(tmp_path))
    x = data.batch("train", 3, 16)
    assert x.shape == (3, 17)
    assert torch.all((x[:, 1:] - x[:, :-1]) % 256 == 1)


def test_hybrid_drafting_matches_plain_decoding():
    # with greedy sampling, draft-and-verify must write exactly what one-byte-at-a-time decoding writes
    torch.manual_seed(0)
    model = build("hybrid", TINY).eval()
    prompt = torch.tensor([list(b"the quick ")])
    fast = model.generate(prompt, 20, temperature=1e-5, top_k=1)
    slow = super(type(model), model).generate(prompt, 20, temperature=1e-5, top_k=1)
    assert fast.tolist() == slow.tolist()
    assert model.last_generate_stats["passes"] <= 21


def test_hybrid_eval_loss_is_backbone_only():
    torch.manual_seed(0)
    model = build("hybrid", TINY)
    x = batch()
    model.eval()
    exact = model.loss(x).item()
    model.train()
    model.iterations = TINY.core_iterations
    assert model.loss(x).item() > exact  # training adds the drafter's loss on top
