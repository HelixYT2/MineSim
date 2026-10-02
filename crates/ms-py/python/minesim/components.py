"""Pluggable building blocks for :class:`minesim.MineSimEnv`, in the style of RLGym.

An environment is assembled from five pieces: a :class:`StateMutator` that prepares the arena at
the start of an episode, an :class:`ActionParser` that turns a Gymnasium action into simulator
inputs, an :class:`ObsBuilder` that reads the arena into an observation, a :class:`RewardFunction`,
and a :class:`DoneCondition`. Replacing any one of them redefines the task without touching the
rest. Sensible defaults (a flat spawn, button inputs, a velocity observation, and a "move fast"
reward) are provided so the env is useful out of the box.
"""

from __future__ import annotations

import math
from abc import ABC, abstractmethod

import numpy as np

try:
    from gymnasium import spaces
except ImportError as exc:  # pragma: no cover - exercised only without the optional dep
    raise ImportError("the Gymnasium components require `pip install gymnasium`") from exc

from ._core import Arena

# Inputs handed to ``Arena.step``: the seven buttons followed by an absolute yaw.
Inputs = tuple[bool, bool, bool, bool, bool, bool, bool, float]


def wrap_degrees(deg: float) -> float:
    """Fold an angle into [-180, 180), matching how the game keeps yaw bounded."""
    deg = math.fmod(deg, 360.0)
    if deg >= 180.0:
        deg -= 360.0
    elif deg < -180.0:
        deg += 360.0
    return deg


class StateMutator(ABC):
    """Prepares the arena at the start of an episode."""

    @abstractmethod
    def reset(self, arena: Arena, rng: np.random.Generator) -> None: ...


class ActionParser(ABC):
    """Maps a Gymnasium action to simulator inputs."""

    @abstractmethod
    def space(self) -> spaces.Space: ...

    @abstractmethod
    def parse(self, action, yaw: float) -> Inputs: ...


class ObsBuilder(ABC):
    """Reads the arena into an observation array."""

    @abstractmethod
    def space(self) -> spaces.Space: ...

    def reset(self, arena: Arena) -> None:
        pass

    @abstractmethod
    def build(self, arena: Arena) -> np.ndarray: ...


class RewardFunction(ABC):
    def reset(self, arena: Arena) -> None:
        pass

    @abstractmethod
    def compute(self, arena: Arena, prev_pos: tuple[float, float, float]) -> float: ...


class DoneCondition(ABC):
    def reset(self, arena: Arena) -> None:
        pass

    @abstractmethod
    def terminated(self, arena: Arena) -> bool: ...


class FixedSpawn(StateMutator):
    """Spawn at a fixed point. With ``randomize_yaw`` the facing is drawn uniformly each episode.

    ``effects`` is a list of ``(effect_id, amplifier, duration)`` given at every reset, e.g.
    ``[("minecraft:speed", 1, 1000)]``.
    """

    def __init__(self, x=0.5, y=0.0, z=0.5, yaw=0.0, randomize_yaw=False, effects=()):
        self.x, self.y, self.z = float(x), float(y), float(z)
        self.yaw = float(yaw)
        self.randomize_yaw = bool(randomize_yaw)
        self.effects = [tuple(e) for e in effects]

    def reset(self, arena: Arena, rng: np.random.Generator) -> None:
        yaw = float(rng.uniform(-180.0, 180.0)) if self.randomize_yaw else self.yaw
        arena.reset(self.x, self.y, self.z, yaw)
        for effect in self.effects:
            arena.add_effect(*effect)


class ButtonsAction(ActionParser):
    """``MultiBinary(7)``: forward, back, left, right, jump, sprint, sneak. Yaw is held constant."""

    def space(self) -> spaces.Space:
        return spaces.MultiBinary(7)

    def parse(self, action, yaw: float) -> Inputs:
        a = np.asarray(action).astype(bool)
        return (
            bool(a[0]),
            bool(a[1]),
            bool(a[2]),
            bool(a[3]),
            bool(a[4]),
            bool(a[5]),
            bool(a[6]),
            yaw,
        )


class ButtonsTurnAction(ActionParser):
    """``MultiDiscrete``: the seven buttons plus a yaw-turn bucket applied as a per-tick delta."""

    def __init__(self, turn_deltas=(-15.0, -5.0, 0.0, 5.0, 15.0)):
        self.turn_deltas = tuple(float(d) for d in turn_deltas)

    def space(self) -> spaces.Space:
        return spaces.MultiDiscrete([2, 2, 2, 2, 2, 2, 2, len(self.turn_deltas)])

    def parse(self, action, yaw: float) -> Inputs:
        a = np.asarray(action).astype(int)
        new_yaw = wrap_degrees(yaw + self.turn_deltas[int(a[7])])
        return (
            bool(a[0]),
            bool(a[1]),
            bool(a[2]),
            bool(a[3]),
            bool(a[4]),
            bool(a[5]),
            bool(a[6]),
            new_yaw,
        )


class DefaultObs(ObsBuilder):
    """Velocity, ground contact, and the jump cooldown — enough to learn sprint-jump locomotion."""

    _LOW = np.array([-10.0, -10.0, -10.0, 0.0, 0.0], dtype=np.float32)
    _HIGH = np.array([10.0, 10.0, 10.0, 1.0, 1.0], dtype=np.float32)

    def space(self) -> spaces.Space:
        return spaces.Box(self._LOW, self._HIGH, dtype=np.float32)

    def build(self, arena: Arena) -> np.ndarray:
        vx, vy, vz = arena.vel()
        return np.array(
            [vx, vy, vz, float(arena.on_ground()), arena.jump_cooldown() / 10.0],
            dtype=np.float32,
        )


class PlayerObs(ObsBuilder):
    """The player's full movement state: velocity, look direction (as sin/cos of yaw and pitch),
    ground and wall contact, the jump cooldown, sprinting/crouching/swimming, fluid contact, fall
    distance, health and food. Positions are left out so policies do not memorise coordinates;
    wrap or extend this to add task-specific features such as a goal offset."""

    _N = 17

    def space(self) -> spaces.Space:
        return spaces.Box(-np.inf, np.inf, shape=(self._N,), dtype=np.float32)

    def build(self, arena: Arena) -> np.ndarray:
        vx, vy, vz = arena.vel()
        yaw = math.radians(arena.yaw())
        pitch = math.radians(arena.pitch())
        return np.array(
            [
                vx,
                vy,
                vz,
                math.sin(yaw),
                math.cos(yaw),
                math.sin(pitch),
                float(arena.on_ground()),
                float(arena.horizontal_collision()),
                arena.jump_cooldown() / 10.0,
                float(arena.sprinting()),
                float(arena.crouching()),
                float(arena.swimming()),
                float(arena.in_water()),
                float(arena.in_lava()),
                min(arena.fall_distance(), 64.0) / 64.0,
                arena.health() / 20.0,
                arena.food() / 20.0,
            ],
            dtype=np.float32,
        )


class SpeedReward(RewardFunction):
    """Horizontal distance covered this tick — rewards moving as fast as possible."""

    def compute(self, arena: Arena, prev_pos: tuple[float, float, float]) -> float:
        x, _y, z = arena.pos()
        return float(math.hypot(x - prev_pos[0], z - prev_pos[2]))


class NeverDone(DoneCondition):
    """No terminal state; episodes end only by the env's tick limit (truncation)."""

    def terminated(self, arena: Arena) -> bool:
        return False


class Died(DoneCondition):
    """Terminal once the player's health reaches zero (fall damage, lava, hits)."""

    def terminated(self, arena: Arena) -> bool:
        return arena.is_dead()


class AnyOf(DoneCondition):
    """Terminal when any of the given conditions is."""

    def __init__(self, *conditions: DoneCondition):
        self.conditions = conditions

    def reset(self, arena: Arena) -> None:
        for c in self.conditions:
            c.reset(arena)

    def terminated(self, arena: Arena) -> bool:
        return any(c.terminated(arena) for c in self.conditions)


class FellBelow(DoneCondition):
    """Terminal once the player drops past ``y_min`` — useful for void or platform tasks."""

    def __init__(self, y_min=-64.0):
        self.y_min = float(y_min)

    def terminated(self, arena: Arena) -> bool:
        return arena.pos()[1] < self.y_min
