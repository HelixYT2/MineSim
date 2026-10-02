# MineSim

A headless reimplementation of Minecraft Java Edition 1.21.11's server-side simulation,
built for reinforcement learning. No rendering and no networking — just the tick loop, so
many instances can run in parallel far faster than real time to collect rollouts.

The aim is byte-for-byte agreement with the real game's per-tick state, checked by replaying
recorded inputs through both and comparing. `docs/contract.md` defines exactly what that
agreement means. `docs/clean-room-policy.md` describes the implementation boundary: MineSim
reimplements behavior and does not copy or redistribute Mojang code.

## Status

Every subsystem below is checked against the oracle corpus: 38 scenarios recorded from the real
1.21.11 client by the probe mod in `tools/minesim-mod` (`docs/corpus.md`). `cargo xtask oracle`
replays them all and reports, per scenario, how many ticks reproduce the game's complete player
state bit-for-bit. Currently **35 of 38 scenarios (8,431 of 8,680 ticks) are exact**, and CI locks
every exact scenario's hash so a regression fails the build.

| Subsystem | State |
| --- | --- |
| Walking, sprinting (with the game's sprint rules, double-tap and hunger), sneaking with edge back-off, crouching under low ceilings, jumping | bit-exact |
| Collision with step-up on every block shape (slabs, stairs, fences, walls, panes, doors, trapdoors, snow layers, ...) | bit-exact |
| Block friction and speed/jump factors (ice, packed and blue ice, slime, honey, soul sand) | bit-exact |
| Slime and bed bounces, honey wall slide, cobweb and berry-bush slowdown | bit-exact |
| Ladders, vines, twisting vines, scaffolding | bit-exact |
| Water and lava: buoyancy, swimming, sprint-swimming, currents, bubble columns, lava ignition | bit-exact |
| Status effects and attributes: speed, slowness, jump boost, slow falling, levitation, fire resistance, ... | bit-exact |
| Fall damage, damage with the invulnerability window, knockback (including the server's copy of the velocity and the network quantization the client receives) | bit-exact |
| Projectiles: snowballs, eggs, ender pearls, arrows, spectral arrows (flight, block hits, arrows sticking, hitting the player) | bit-exact on every recorded sample |
| `java.util.Random`, `LegacyRandomSource`, Xoroshiro128++, `Mth`, fdlibm math | bit-exact vs the JVM and the game classes |
| Freezing in powder snow, magma-block damage timing | close, not yet exact (the 3 remaining scenarios) |
| Mobs, redstone, world generation, riding, elytra, creative flight | out of scope |

An agent's action is what a player at the keyboard controls: the seven movement keys and the look
direction. Sprinting and crouching follow from them by the game's own rules.

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

Worlds, effects, damage and projectiles:

```python
blocks = [(0, y, 3, "minecraft:stone") for y in range(5)] + \
         [(0, y, 2, "minecraft:ladder[facing=north]") for y in range(5)]
arena = minesim.Arena(surface_y=0, blocks=blocks)       # or arena.set_block(x, y, z, state)
arena.add_effect("speed", amplifier=1, duration=600)    # Speed II for 30 s
arena.hurt(2.0, from_x=0.5, from_z=5.0)                 # a hit with knockback away from (0.5, 5)
arena.spawn_projectile("arrow", 0.5, 1.2, -6.0, 0.0, 0.05, 2.0)
arena.step(forward=True, yaw=0.0, pitch=0.0)
print(arena.health(), arena.pose(), arena.in_water(), arena.effects(), arena.state_dict())
snapshot = arena.get_state(); arena.set_state(snapshot)  # exact checkpoint/restore
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
states = batch.states()        # (8192, 18): see minesim.Batch.STATE_COLUMNS
```

Loading a real save is optional and used mainly for validation: `minesim.Arena(region_dir=...)`.

## Performance

A flat-world sprint-jump workload, measured with `cargo xtask bench` on a 4-core machine:

| | throughput |
| --- | --- |
| one environment, one thread | ~0.9M ticks/s (~45,000x real time) |
| 4096 environments across the pool | ~1.7M env-ticks/s |

Each tick runs everything the game does for the player (fluid and climbing checks, the supporting
block, pose fitting, inside-block effects, the server's copy of the player), which is why this is
slower than the earlier walking-only kernel.

## Layout

| Crate | Purpose |
| --- | --- |
| `ms-numerics` | IEEE-754 / fdlibm math, the game's `Mth` tables and functions |
| `ms-rng` | `java.util.Random`, `LegacyRandomSource`, Xoroshiro128++, the game's seeding |
| `ms-data` | generated block registry, properties, collision shapes, fluids, tags |
| `ms-world` | flat worlds, built worlds (`GridWorld`), and Anvil region loading |
| `ms-kernel` | the player tick: input, movement, collision, fluids, climbing, effects, damage, projectiles |
| `ms-arena` | the public simulation API, single and batched |
| `ms-oracle` | the canonical state hash (`docs/contract.md`) |
| `ms-corpus` | the oracle corpus loader, replay driver and golden-hash lock |
| `ms-py` | Python bindings (Gymnasium env and components) |
| `xtask` | developer tasks |

## Building

```
cargo test                                       # the Rust workspace
maturin develop -m crates/ms-py/Cargo.toml       # build and install the Python extension
python crates/ms-py/tests/test_minesim.py        # the binding tests
cargo xtask bench                                 # throughput benchmark
cargo xtask oracle                                # replay the corpus, per-scenario exactness
```

Uses the toolchain pinned in `rust-toolchain.toml`. Java 21 is needed only to regenerate the
reference data and record the traces the simulator is validated against (`tools/minesim-mod`,
`tools/refgen`), not to build or run it.
