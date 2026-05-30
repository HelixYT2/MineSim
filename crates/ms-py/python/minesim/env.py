"""A Gymnasium environment over the MineSim arena.

The default task is "move as far as you can" on a flat stone world: button inputs, a velocity
observation, and a reward equal to the horizontal distance covered each tick. Every part of the
task is a swappable component (see :mod:`minesim.components`), so the same env drives anything
from sprint-jump locomotion to navigation once richer observations and rewards are plugged in.
"""

from __future__ import annotations

try:
    import gymnasium as gym
except ImportError as exc:  # pragma: no cover - exercised only without the optional dep
    raise ImportError("MineSimEnv requires `pip install gymnasium`") from exc

from ._core import Arena
from .components import (
    ButtonsAction,
    DefaultObs,
    FixedSpawn,
    NeverDone,
    SpeedReward,
)


class MineSimEnv(gym.Env):
    """A single-agent Minecraft-movement environment.

    Parameters mirror the arena: pass ``region_dir`` to play on a real save, otherwise a flat
    world with its surface at ``surface_y`` (floored with ``floor_block``, default stone) is used.
    The five task components default to the "move fast" task and can each be overridden.
    """

    metadata = {"render_modes": []}

    def __init__(
        self,
        *,
        region_dir=None,
        surface_y=0,
        floor_block=None,
        spawn=(0.5, 0.0, 0.5),
        yaw=0.0,
        obs_builder=None,
        action_parser=None,
        reward_fn=None,
        done_condition=None,
        state_mutator=None,
        max_episode_ticks=200,
    ):
        super().__init__()
        self.render_mode = None
        x, y, z = spawn
        self._arena = Arena(
            region_dir=region_dir,
            surface_y=surface_y,
            floor_block=floor_block,
            x=x,
            y=y,
            z=z,
            yaw=yaw,
        )
        self.obs_builder = obs_builder or DefaultObs()
        self.action_parser = action_parser or ButtonsAction()
        self.reward_fn = reward_fn or SpeedReward()
        self.done_condition = done_condition or NeverDone()
        self.state_mutator = state_mutator or FixedSpawn(x, y, z, yaw=yaw)
        self.max_episode_ticks = int(max_episode_ticks)

        self.observation_space = self.obs_builder.space()
        self.action_space = self.action_parser.space()

        self._yaw = float(yaw)
        self._ticks = 0

    def reset(self, *, seed=None, options=None):
        super().reset(seed=seed)
        self.state_mutator.reset(self._arena, self.np_random)
        self._yaw = self._arena.yaw()
        self._ticks = 0
        self.obs_builder.reset(self._arena)
        self.reward_fn.reset(self._arena)
        self.done_condition.reset(self._arena)
        return self.obs_builder.build(self._arena), {}

    def step(self, action):
        prev_pos = self._arena.pos()
        f, b, left, right, jump, sprint, sneak, yaw = self.action_parser.parse(action, self._yaw)
        self._yaw = yaw
        self._arena.step(f, b, left, right, jump, sprint, sneak, yaw)
        self._ticks += 1

        obs = self.obs_builder.build(self._arena)
        reward = self.reward_fn.compute(self._arena, prev_pos)
        terminated = self.done_condition.terminated(self._arena)
        truncated = self._ticks >= self.max_episode_ticks
        return obs, reward, terminated, truncated, {}

    def state_hash(self) -> int:
        """The arena's canonical per-tick state hash (see ``docs/contract.md``)."""
        return self._arena.state_hash()

    def render(self):
        return None

    def close(self):
        pass


def make_vec_env(num_envs=8, **kwargs):
    """A synchronous vector of independent :class:`MineSimEnv` instances.

    Each env is fully isolated, so this is deterministic and embarrassingly parallel. (A native
    batched stepper for higher throughput lives in the Rust core.)
    """
    from gymnasium.vector import SyncVectorEnv

    return SyncVectorEnv([lambda: MineSimEnv(**kwargs) for _ in range(num_envs)])
