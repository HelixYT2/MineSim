# minesim

Python bindings for [MineSim](https://github.com/HelixYT2/MineSim) — a bit-exact, native-Rust
reimplementation of Minecraft Java 1.21.11 movement physics, built for reinforcement learning.

```python
import minesim

# A flat stone world; the player spawns at rest on the surface.
arena = minesim.Arena(surface_y=0, x=0.5, y=0.0, z=0.5)
for _ in range(20):
    arena.step(forward=True, sprint=True, jump=True)
print(arena.pos(), arena.vel(), arena.on_ground())
```

A Gymnasium environment with pluggable observation/action/reward components is available when
`gymnasium` is installed:

```python
import gymnasium, minesim
env = minesim.MineSimEnv()          # flat world, "move fast" task by default
obs, info = env.reset(seed=0)
obs, reward, terminated, truncated, info = env.step(env.action_space.sample())
```

There is no Java runtime or Minecraft installation required. Loading a real save is optional
(`minesim.Arena(region_dir=...)`) and is used mainly to validate the physics against vanilla.
