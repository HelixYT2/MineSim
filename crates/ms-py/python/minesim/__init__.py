"""MineSim: a bit-exact Minecraft 1.21.11 movement simulator for reinforcement learning.

``minesim.Arena`` is the raw simulator and needs nothing but this package. The Gymnasium
environment (``minesim.MineSimEnv``) and its components are only loaded if ``gymnasium`` is
installed, so the core stays importable in dependency-light settings.
"""

from ._core import Arena, Batch, __version__

__all__ = ["Arena", "Batch", "__version__"]

try:
    import gymnasium as _gymnasium  # noqa: F401
except ImportError:
    pass
else:
    from . import components
    from .env import MineSimEnv, make_vec_env

    __all__ += ["MineSimEnv", "make_vec_env", "components"]
