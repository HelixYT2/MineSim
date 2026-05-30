# MineSim

A headless reimplementation of Minecraft Java Edition 1.21.11's server-side simulation,
built for reinforcement learning. No rendering and no networking — just the tick loop, so
many instances can run in parallel far faster than real time to collect rollouts.

The aim is byte-for-byte agreement with the real game's per-tick state, checked by replaying
recorded inputs through both and comparing. `docs/contract.md` defines exactly what that
agreement means. `docs/clean-room-policy.md` describes the implementation boundary: MineSim
reimplements behavior and does not copy or redistribute Mojang code.

## Scope

The first target is player movement, collision, knockback, status effects, and projectiles
on static terrain, exposed through a Gymnasium-compatible Python API with no Java runtime
dependency. Fluids, redstone, mobs, and world generation come later.

## Layout

| Crate | Purpose |
| --- | --- |
| `ms-numerics` | IEEE-754 / fdlibm math and the game's trig tables |
| `ms-rng` | `java.util.Random` and Xoroshiro128++ |
| `ms-data` | generated block, registry, and collision tables |
| `ms-world` | chunk storage and region/NBT loading |
| `ms-kernel` | tick loop, movement, collision, physics |
| `ms-arena` | the public simulation API |
| `ms-oracle` | state hashing and the differential test harness |
| `ms-py` | Python bindings |
| `xtask` | developer tasks |

## Building

```
cargo test
```

Uses the toolchain pinned in `rust-toolchain.toml`. Java 21 is needed to generate the
reference data and traces the simulator is validated against.
