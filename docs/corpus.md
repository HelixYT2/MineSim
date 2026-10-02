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

`fall` is the game's `double fallDistance`. The canonical hash of this state (which fields, in which
order, `age` included) is defined in `docs/contract.md` §2.0.

## Replaying

A replay rebuilds the world from the header, initialises the simulator from row 0's `pre`, and
for each row: applies the `pre` diff (the external changes), steps one tick with `in`, and
compares the result with `post` field by field. The driver is `ms_corpus::replay`:

```rust
let scenario = ms_corpus::Scenario::load("walk_basic")?;
let report = ms_corpus::replay(&scenario, |p, input, world| ms_kernel::player::tick(p, input, world));
```

`replay` free-runs: apart from the recorded `pre` diffs the simulated state is never touched, so an
error compounds exactly as it would in a real run. `replay_with(&scenario, Mode::Resync, ..)` also
overwrites the state with the recorded `post` after every tick that diverged, which judges each tick
from a correct start (useful while a kernel is mostly wrong; it is best-effort for fields the
kernel owns, such as attribute modifiers). The `Report` holds, per scenario:

| field | meaning |
| --- | --- |
| `ticks`, `exact_ticks` | recorded ticks, and ticks whose end state matched in every compared field |
| `longest_streak`, `longest_streak_start` | longest run of consecutive exact ticks and where it starts |
| `first_divergence` | the first tick that differs, the first differing field (in the contract's field order, `docs/contract.md` §2.0) and both values decoded; how many fields differ at that tick |
| `diffs` | per tick, every differing field (attributes as `attrs.<name>`, effects readable) |
| `sim_hashes`, `oracle_hashes` | `H(t)` of the simulated and of the recorded end-of-tick state (contract §3); `hash_exact_ticks()` counts the ticks where they are equal, `sim_rolling()` / `oracle_rolling()` fold them into the scenario's rolling hash |

A tick is *exact* when every compared field matches bit-for-bit. The compared fields are the
recorded ones except the derived queries `climbable` and `fallFlying`; the lifetime tick count `age`
*is* compared by the replay (it is part of the hashed state), although `compare_state` itself still
skips it.

`ms_corpus::projectiles` does the same for the server-side projectile events: `projectile_tracks`
groups the `srv` samples per entity id together with the `spawn` action's initial state, and
`replay_projectiles(&scenario, |projectile, ctx| ..)` steps each projectile one server tick per
sample from its spawn state and compares it with the recording (the step function is the
projectile module's own; `ctx` carries the world and the recorded player state of the row).

### `cargo xtask oracle`

```
cargo xtask oracle                          # table over every scenario (free-run)
cargo xtask oracle walk_basic water_pool    # only those scenarios
cargo xtask oracle walk_basic --verbose     # per-tick field diffs for one scenario
                    [--from TICK] [--limit N]
cargo xtask oracle --resync                 # judge each tick from a correct start
cargo xtask oracle --strict                 # exit status 1 unless every scenario is bit-exact
cargo xtask oracle --projectiles            # also list the recorded projectiles
cargo xtask oracle --bless [names]          # (re)write corpus/golden-hashes.json
cargo xtask oracle --bless --oracle-only    # corpus fingerprints only, without the kernel
```

The table lists, per scenario, the recorded ticks, the exact ticks, the longest exact streak
(`length@start tick`) and the first divergence (`t=<tick> <field>: expected .., actual ..`). The exit
status is zero unless `--strict` is given (or the arguments are bad), so the command is safe to run
as an informational step.

## The golden-hash lock

`corpus/golden-hashes.json` is the committed lock of `docs/contract.md` §5. For every scenario it
holds:

- `oracle` — the rolling hash of the *recorded* states. It fingerprints the corpus file and the
  contract serialization; it is independent of the kernel and must always match, so re-recording a
  scenario (or touching the serialization) fails the test until the file is re-blessed.
- `checkpoints` — the rolling hash (high 32 bits) after every 16 ticks, so a failure can name the
  window of ticks that changed.
- `sim` — the rolling hash of the states the kernel produced when blessed; `frozen` — whether that
  equalled `oracle`, i.e. the kernel reproduced the scenario bit-for-bit.

`cargo test` (`crates/ms-corpus/tests/golden.rs`, also run by CI) replays every scenario through
`ms_kernel::player::tick` and applies the rule:

| situation | result |
| --- | --- |
| scenario has no entry | passes, prints how to bless — **unless** the file is `locked` |
| recorded states no longer match `oracle` | fails (corpus or serialization changed) |
| entry is `frozen` and the kernel's hash differs | **fails**, naming the first diverging tick and field |
| entry is not frozen and the kernel is now exact | passes, prints that it can be frozen |
| entry is not frozen and the kernel's hash changed | passes (fails with `MINESIM_GOLDEN_STRICT=1`) |

### Freezing

The freeze rule (contract §5) in practice:

1. Land kernel work; run `cargo xtask oracle` and look at the table.
2. When scenarios are bit-exact, `cargo xtask oracle --bless` (all scenarios; or name the ones to
   refresh) records their hashes as `frozen`. Once every corpus scenario has a blessed hash the
   file becomes `locked`: from then on a corpus scenario without an entry fails too.
3. Commit `corpus/golden-hashes.json` together with the change. From now on CI fails any change that
   moves a frozen scenario's hash. If the change is intended (it must then be a *fix* of the
   reference, since frozen means bit-exact with the real game), re-bless in the same review.

A file blessed with `--oracle-only` locks the corpus fingerprints before the kernel is exact. The
file records the contract version and hash seed it was written for; a new contract version
(`docs/contract.md` §6) requires re-blessing everything and the lock refuses to compare across
versions.

### CI

CI runs `cargo test --workspace --exclude ms-py`, which includes the corpus tests and the golden
lock, on Linux, Windows and macOS (ARM64), so a frozen scenario must hash identically on every
platform. The corpus is stored as plain `corpus/client/*.jsonl.gz` files (no LFS: a clone has the
data, there is nothing to fetch); CI checks that each is a real gzip stream and prints the
conformance table.
