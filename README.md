# MineSim

A headless reimplementation of Minecraft Java Edition 1.21.11's server-side simulation,
built for reinforcement learning. No rendering and no networking — just the tick loop, so
many instances can run in parallel far faster than real time to collect rollouts.

The aim is byte-for-byte agreement with the real game's per-tick state, checked by replaying
recorded inputs through both and comparing. `docs/contract.md` defines exactly what that
agreement means. `docs/clean-room-policy.md` describes the implementation boundary: MineSim
reimplements behavior and does not copy or redistribute Mojang code.

## Status

| Subsystem | State |
| --- | --- |
| Player movement, jumping, sprinting, sneaking | bit-exact, validated against recorded play |
| Per-axis AABB / VoxelShape collision and step-up on static terrain | bit-exact |
| Block friction (ice, slime, ...) | bit-exact |
| `java.util.Random`, `Mth` sin/cos tables | bit-exact vs the JVM |
| Knockback, damage, status effects, projectiles | planned |
| Fluids, ladders, redstone, mobs, world generation | out of scope for now |

Movement is validated by replaying a recorded session through both the game and MineSim: every
grounded-walking tick that lands on static terrain reproduces the game's position and velocity
bit-for-bit, and a recorded run free-runs through the simulator for ~2000 consecutive ticks with
zero drift. The few remaining mismatches are entity collisions (boats, shulkers), which the
movement core does not yet model.

## Python

```
pip install minesim          # the wheel bundles the Rust core; no Java needed
```

Raw simulator:

```python
import minesim

arena = minesim.Arena(surface_y=0)        # a flat stone world
for _ in range(20):
    arena.step(forward=True, sprint=True, jump=True)
print(arena.pos(), arena.vel(), arena.on_ground())
```

Gymnasium environment (the default task rewards horizontal distance, so agents learn to
sprint-jump). Every part of the task — observation, action, reward, termination, reset — is a
swappable component; see `minesim.components`.

```python
import gymnasium, minesim

env = minesim.MineSimEnv()
obs, info = env.reset(seed=0)
obs, reward, terminated, truncated, info = env.step(env.action_space.sample())
```

Thousands of independent environments step together natively, with the physics running across a
thread pool and the GIL released:

```python
import numpy as np, minesim

batch = minesim.Batch(num_envs=8192, surface_y=0)
actions = np.zeros((8192, 8)); actions[:, [0, 4, 5]] = 1   # forward + jump + sprint
batch.step(actions)
states = batch.states()        # (8192, 9): position, velocity, yaw, on-ground, jump cooldown
```

Loading a real save is optional and used mainly for validation: `minesim.Arena(region_dir=...)`.

## Performance

A flat-world sprint-jump workload, measured with `cargo xtask bench`:

| | throughput |
| --- | --- |
| one environment, one thread | ~8M ticks/s (~400,000x real time) |
| 4096 environments across the pool | ~40M env-ticks/s |

## Layout

| Crate | Purpose |
| --- | --- |
| `ms-numerics` | IEEE-754 / fdlibm math and the game's trig tables |
| `ms-rng` | `java.util.Random` and Xoroshiro128++ |
| `ms-data` | generated block, registry, and collision tables |
| `ms-world` | chunk storage, region/NBT loading, and synthetic flat worlds |
| `ms-kernel` | tick loop, movement, collision, physics |
| `ms-arena` | the public simulation API, single and batched |
| `ms-oracle` | state hashing and the differential test harness |
| `ms-py` | Python bindings (Gymnasium env and components) |
| `xtask` | developer tasks |

## Building

```
cargo test                                       # the Rust workspace
maturin develop -m crates/ms-py/Cargo.toml       # build and install the Python extension
python crates/ms-py/tests/test_minesim.py        # the binding tests
cargo xtask bench                                 # throughput benchmark
```

Uses the toolchain pinned in `rust-toolchain.toml`. Java 21 is needed only to regenerate the
reference data and traces the simulator is validated against, not to build or run it.
