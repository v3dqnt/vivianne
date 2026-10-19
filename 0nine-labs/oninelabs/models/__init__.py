"""The Viv model family. Every member reads bytes and shares one interface (see common.ByteModel)."""

from .common import Config, ByteModel
from .diffusion import Diffusion
from .hybrid import Hybrid
from .looped import Looped
from .ssm import SSM
from .transformer import Transformer

FAMILY: dict[str, type[ByteModel]] = {
    "transformer": Transformer,  # Viv-T, the reference
    "ssm": SSM,  # Viv-S, selective state space
    "looped": Looped,  # Viv-L, recurrent depth
    "diffusion": Diffusion,  # Viv-D, masked discrete diffusion
    "hybrid": Hybrid,  # Viv-H, S + T + L + D in one model
}


def build(name: str, cfg: Config) -> ByteModel:
    if name not in FAMILY:
        raise KeyError(f"unknown model {name!r}; the family is {', '.join(FAMILY)}")
    return FAMILY[name](cfg)


__all__ = ["FAMILY", "ByteModel", "Config", "build"]
