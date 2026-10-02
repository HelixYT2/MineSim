# The oracle corpus

The corpus is the ground truth MineSim is checked against: traces recorded from the real
Minecraft 1.21.11 client, driven through scripted scenarios by the trace mod in
`tools/minesim-mod`. Each scenario builds a small arena, places the player, then plays a fixed
input program while logging the player's complete physics state every tick.

## Recording

```
cd tools/minesim-mod
./gradlew runOracle                       # every scenario; add -Pscenarios=a,b to narrow
xvfb-run -a ./gradlew runOracle           # the same, headless (Linux)
```

`runOracle` joins the singleplayer world `run/saves/oracle` (a superflat world with a single stone
layer at y = -64, cheats on), runs the scenarios defined in
`src/client/java/minesim/client/Scenarios.java`, writes `run/minesim-corpus/client/<name>.jsonl`,
and quits. `-Pdumpblocks=true` additionally writes `run/minesim-blocks.json` for
`cargo xtask regen-data`.

## File format (`minesim-oracle/1`)

Every float and double is its raw IEEE-754 bit pattern as a JSON integer (`f32` → `i32` bits,
`f64` → `i64` bits), so values round-trip exactly.

**Line 1 — header.**

| key | meaning |
| --- | --- |
| `scenario`, `description` | name and what it exercises |
| `arena` | the box the scenario owns: `floorY` (-64), `floorBlock`, `minX..maxX`, `minZ..maxZ`, `maxY` |
| `blocks` | every block in the arena that differs from the base world, as `[x, y, z, "block[props]"]`, dumped *after* the world settled (so flowing water is as it really was) |
| `build` | the fills the scenario requested (informational; `blocks` is authoritative) |
| `start` | requested start position/rotation, food, health, initial effects |
| `ticks`, `settle` | recorded ticks; ticks the world was left to settle before recording |

The base world is stone at y = -64 (top face -63) and air above, everywhere, including outside
the arena box. Overriding blocks come only from `blocks`.

**Lines 2.. — one row per client tick.**

| key | meaning |
| --- | --- |
| `t` | tick index from 0 |
| `in` | the input for this tick: keys `f b l r j s sp` (forward, back, left, right, jump, sneak, sprint; 0/1) and `yaw`, `pitch` (f32 bits) |
| `pre` | the player state at the start of the tick, before its physics. Row 0 holds the full state; later rows hold only the fields that differ from the previous row's `post` — i.e. what the *server* changed between ticks (knockback velocity, teleports, effects, health). |
| `post` | the full player state after the tick |
| `srv` | optional: server-side events that ran before this tick (`kind: "action"` with the action's own before/after log, or `kind: "projectiles"` with every projectile's server state at a server tick) |

**Player state fields** (`pre`/`post`): `x y z` position, `dx dy dz` velocity, `yaw pitch`;
`ground hc mhc vc vcb` (on ground, horizontal / minor-horizontal / vertical / vertical-below
collision); `fall` fall distance (f64); `water eyeWater lava` and `waterH lavaH` fluid state;
`powder wasPowder`; `stuckX stuckY stuckZ` (stuck-speed multiplier); `support` (packed
`BlockPos.asLong` of the supporting block, absent if none) and `noBlocks`; `pose`, `w h` (box,
f32); `sprinting shift swimming crouching flying`; `fire frozen age invul`; `health absorption
hurtTime lastHurt deathTime`; `njd` (no-jump delay), `jumping`, `xxa zza` (movement input, f32),
`speed` (f32); `climbable fallFlying`; `effects` (`id amp dur`); `attrs` (attribute values, f64);
`food saturation exhaustion jumpTrigger sprintTrigger`.

## Replaying

A replay rebuilds the world from the header, initialises the simulator from row 0's `pre`, and
for each row: applies the `pre` diff (the external changes), steps one tick with `in`, and
compares the result with `post` field by field. `cargo xtask oracle` reports, per scenario, the
first tick and field that differ.
