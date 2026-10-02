# MineSim Bit-Exact Contract

**Status:** `contract-v1` — **frozen**. The player-state layout in §2.0 is final; changing it requires
a new contract version (§6). Tag the commit that introduces this revision `contract-v1`.
**Target:** Minecraft Java Edition **1.21.11** (Java 21 runtime semantics)
**Reference implementations:** the simulator side is `ms_oracle::player` (`crates/ms-oracle`), the
recorded-corpus side is `ms_corpus::canonical` (`crates/ms-corpus`); the replay, report and lock
tooling is described in `docs/corpus.md`.

This document is the **authoritative definition of correctness** for MineSim. Every
acceptance gate is stated in terms of this contract. It is
deliberately mechanical: correctness is byte equality, not human judgement.

---

## 1. Definition

A MineSim run is **bit-exact** with vanilla over an input corpus *iff*, for every tick
`t ∈ [0, N)`, the canonical per-tick **state hash** `H(t)` computed by MineSim equals the
hash of the state the oracle trace mod recorded from the real game for the **same** scripted inputs,
world, and gamerule/configuration (§2, §3) — for all `t`.

There is **no epsilon and no ULP tolerance.** A single differing bit at any tick is a
failure. This is a deliberate, project-wide decision: Minecraft state is recursive across
ticks (`pos(t+1)` is a function of `pos(t)`), so any rounding difference, however small,
will eventually cross a collision boundary or a comparison branch and fork the trajectory.

A subsystem that cannot (yet) be made bit-exact is **deferred**, not approximated. It does
not ship until it is provable.

---

## 2. Canonical state and its serialization

The byte layout below **is** the contract. Both sides must produce identical bytes for identical
state: the simulator serializes its `PlayerState` (`ms_oracle::player::serialize_player`), and the
oracle side serializes the state the real client recorded (`ms_corpus::canonical`, which reads the
raw bit patterns the trace mod writes into the corpus, `docs/corpus.md`). The two are independent
implementations, checked against each other on every recorded tick of the corpus. The trace mod
therefore does not need to hash anything itself: the corpus keeps the raw fields, which is also what
the field-level diff needs.

All multi-byte integers are **big-endian**. Field order is fixed (see §2.0).

| Field class | Source type (Java) | Serialization |
|---|---|---|
| Position `x,y,z` | `double` | `Double.doubleToRawLongBits` → `u64` BE (Vec3 = 3×u64) |
| Velocity `dx,dy,dz` | `double` | raw `u64` BE |
| `fallDistance` | `double`¹ | raw `u64` BE |
| Rotation `yaw,pitch` | `float` | `Float.floatToRawIntBits` → `u32` BE |
| Any `float` game field | `float` | raw `u32` BE — **never** promote to f64 before hashing |
| Movement inputs `xxa,zza` | `float` | raw `u32` BE |
| Attribute values | `double` (`getValue()`) | raw `u64` BE |
| Booleans/flags² | `boolean` | `u8` (`0x00`/`0x01`) |
| Integer fields | `int`/`long` | wrapping two's-complement, raw `i32`/`i64` BE |

¹ `fallDistance` is a **`double`** in 1.21.11 (`public double fallDistance` in `Entity`, checked
against the decompiled reference). It is recorded and hashed as raw `f64` bits.
² The flag set of `contract-v1` is listed in §2.0. `noPhysics` is not part of v1: the simulated
survival player never has it set, so it would be a constant byte. Adding it later is a new
contract version.

### 2.0 The `contract-v1` player record (frozen)

The serialized state of the local player at the **end of a client tick** is the version byte
`0x01` followed by the fields below, in exactly this order, with no padding or separators. The
"corpus key" is the name the oracle corpus records the field under; the game source is the field
whose value is recorded. The machine-readable form is `ms_oracle::player::LAYOUT`.

| # | corpus key | encoding | game source |
|---|---|---|---|
| 1–3 | `x y z` | f64 | `Entity.position()` |
| 4–6 | `dx dy dz` | f64 | `Entity.getDeltaMovement()` |
| 7–8 | `yaw pitch` | f32 | `getYRot()`, `getXRot()` |
| 9 | `ground` | u8 | `onGround` |
| 10 | `hc` | u8 | `horizontalCollision` |
| 11 | `mhc` | u8 | `minorHorizontalCollision` |
| 12 | `vc` | u8 | `verticalCollision` |
| 13 | `vcb` | u8 | `verticalCollisionBelow` |
| 14 | `fall` | f64 | `fallDistance` (double¹) |
| 15 | `water` | u8 | `wasTouchingWater` (what `isInWater()` returns) |
| 16 | `eyeWater` | u8 | `wasEyeInWater` |
| 17 | `lava` | u8 | `isInLava()` |
| 18–19 | `waterH lavaH` | f64 | `fluidHeight` for water, lava |
| 20 | `powder` | u8 | `isInPowderSnow` |
| 21 | `wasPowder` | u8 | `wasInPowderSnow` |
| 22–24 | `stuckX stuckY stuckZ` | f64 | `stuckSpeedMultiplier` |
| 25 | `support` | u8 + i64 | `mainSupportingBlockPos`: presence byte, then `BlockPos.asLong` (0 if absent) |
| 26 | `noBlocks` | u8 | `onGroundNoBlocks` |
| 27 | `pose` | u8 | `Pose.id()` (`STANDING` 0, `FALL_FLYING` 1, `SLEEPING` 2, `SWIMMING` 3, `SPIN_ATTACK` 4, `CROUCHING` 5, `LONG_JUMPING` 6, `DYING` 7, ...) |
| 28–29 | `w h` | f32 | bounding-box width and height (derived from pose and the `scale` attribute; hashed because collision depends on it) |
| 30 | `sprinting` | u8 | `isSprinting()` |
| 31 | `shift` | u8 | `isShiftKeyDown()` |
| 32 | `swimming` | u8 | `isSwimming()` |
| 33 | `fire` | i32 | `remainingFireTicks` |
| 34 | `frozen` | i32 | `ticksFrozen` |
| 35 | `age` | i32 | `tickCount` |
| 36 | `invul` | i32 | `invulnerableTime` |
| 37–38 | `health absorption` | f32 | `getHealth()`, `getAbsorptionAmount()` |
| 39 | `hurtTime` | i32 | `hurtTime` |
| 40 | `lastHurt` | f32 | `lastHurt` |
| 41 | `deathTime` | i32 | `deathTime` |
| 42 | `njd` | i32 | `noJumpDelay` |
| 43 | `jumping` | u8 | `jumping` |
| 44–46 | `xxa zza speed` | f32 | `xxa`, `zza` (`yya` is not simulated for the player and is not hashed), `getSpeed()` |
| 47 | `effects` | list | `i32` count, then per active effect in ascending order of id (byte order): `u16` byte length + UTF-8 id (`minecraft:speed`), `i32` amplifier, `i32` remaining duration (`-1` = infinite) |
| 48 | `attrs` | 14 × f64 | `AttributeInstance.getValue()` — the *value*, modifiers included — in the order `movement_speed jump_strength gravity step_height safe_fall_distance fall_damage_multiplier knockback_resistance water_movement_efficiency movement_efficiency sneaking_speed max_health armor scale burning_time` |
| 49 | `food` | i32 | `FoodData` food level |
| 50–51 | `saturation exhaustion` | f32 | `FoodData` saturation and exhaustion |
| 52–53 | `jumpTrigger sprintTrigger` | i32 | `Player.jumpTriggerTime`, `LocalPlayer.sprintTriggerTime` |
| 54–55 | `flying crouching` | u8 | `Abilities.flying`, `LocalPlayer.crouching` |

The length is `328 + Σ (10 + len(id))` bytes over the active effects (383 for the test vector
below). **Not hashed**, by design: the server's shadow copy of the velocity (`PlayerState::
server_vel`; the client never observes it, and its effect reaches `dx dy dz` when knockback is
delivered) and the attribute *modifier* lists (the oracle records values, which is all that
influences anything). The derived queries `climbable` and `fallFlying` that the corpus also
records are checked through their effects, not hashed.

**Test vector.** The state with `x=4.5 y=-63 z=-2.5`, `dx=0 dy=-0.0784000015258789 dz=0.1`,
`yaw=135 pitch=-22.5`, `ground=1 hc=0 mhc=1 vc=1 vcb=1`, `fall=2.5`, `water=0 eyeWater=1 lava=0`,
`waterH=0.8888888955116272 lavaH=0`, `powder=0 wasPowder=1`, `stuck=(0.25, 0.05, 0.25)`,
`support=(4,-64,-3)`, `noBlocks=0`, `pose=CROUCHING`, `w=0.6 h=1.5`, `sprinting=0 shift=1
swimming=0`, `fire=-20 frozen=7 age=1234 invul=12`, `health=17.5 absorption=2`, `hurtTime=4
lastHurt=3`, `deathTime=0 njd=6 jumping=1`, `xxa=f32 bits 0x3e96872c zza=-xxa speed=0.1f`, effects `speed amp 1
dur 600` and `jump_boost amp 0 dur -1`, attribute values `0.5 + i/64` for index `i` of the list
above except `max_health = 20` and `scale = 1`, `food=17 saturation=3.5 exhaustion=1.25`
`jumpTrigger=3 sprintTrigger=0`, `flying=0 crouching=1`, hashes to `0xb5e5734694219ef9`. That
value is pinned by `ms_oracle::player::tests::contract_v1_hash_is_frozen`; it was reproduced by an
independent implementation written from this section.

### 2.1 RNG state (the earliest divergence detector)

*Reserved: not part of the `contract-v1` bytes.* The v1 corpus is the local player's physics, which
consumes no random numbers; hashing generator state starts with the first version that simulates a
consumer. The rules below govern that version.

RNG drift desynchronizes everything downstream and is usually the *first* thing to diverge,
so it is part of the hash:

- **`java.util.Random` (LCG):** the 48-bit seed as `u64` BE, plus `haveNextNextGaussian`
  (`u8`) and `nextNextGaussian` (raw `u64` bits) when the generator is a `java.util.Random`.
- **Xoroshiro128++ (`RandomSource`):** the two 64-bit state words `seedLo`, `seedHi` as
  `u64` BE each.

Every generator instance that influences hashed state (entity RNG, `level.random`, etc.)
contributes its state in a fixed slot.

### 2.2 Block deltas

*Reserved: not part of the `contract-v1` bytes* (the simulated player changes no blocks).

The **ordered** list of block changes applied during the tick:
`(BlockPos packed as i64 BE, prior_state_id u32 BE, new_state_id u32 BE)`, in **application
order**. Order is part of the contract — it encodes the neighbor-update direction orders
(Place/Physics `W,E,N,S,D,U`; neighbor-changed `W,E,D,U,N,S`; comparator `N,E,S,W`).

### 2.3 Entity set membership

*Reserved: not part of the `contract-v1` bytes.* Projectiles are replayed and compared against the
server-side samples (`docs/corpus.md`), field by field, outside the player hash.

Entities are serialized **stable-sorted** by a deterministic key — `(network id, then UUID
bytes)` — so that the *internal* iteration order (which must match vanilla's fastutil order
for correctness, see §4) never leaks into the hash by accident. The hash captures *which
entities exist and their per-field state*; the iteration-order requirement is enforced
separately by the kernel, not by the hash sort.

---

## 3. The hash

```
H(t) = xxh3_64( seed = MINESIM_HASH_SEED, bytes = serialize(state_t) )
```

- `serialize` is the §2.0 record: the version byte, then the fields in the frozen order. Both
  implementations must agree field-for-field.
- `MINESIM_HASH_SEED = 0x4d49_4e45_5349_4d00` (ASCII `MINESIM\0`) is a fixed constant
  (`ms_oracle::HASH_SEED`); changing it is a contract change (§6). The contract version itself is
  stamped into every state as the first serialized byte.
- xxh3 is chosen for speed; it is **not** cryptographic and that is fine — the hash only
  needs to be a stable, collision-resistant-enough fingerprint for equality checks.

**Rolling hash.** A whole run (a scenario) is fingerprinted by folding the per-tick hashes:
`R(-1) = 0`, `R(t) = xxh3_64(seed, be64(R(t-1)) ++ be64(H(t)))`. One value `R(N-1)` therefore
pins every tick of a scenario; a difference at any tick changes every later `R`. The golden lock
(§5) stores these.

**Two comparison modes:**
1. **Hash equality** — fast path for corpus sweeps. `H_minesim(t) == H_oracle(t)` for all `t`,
   where `H_oracle(t)` is the hash of the recorded end-of-tick state (`ms_corpus::canonical`).
2. **Field-level first-divergence diff** — debug path. The harness compares the raw §2.0 fields (not
   just the hash) and reports the earliest tick and the first differing field *in the order of
   §2.0*, with both values decoded. This is the primary bisection tool when a gate fails
   (`cargo xtask oracle <scenario> --verbose`).

NaN is compared **by bit pattern** (raw bits, `Float.floatToRawIntBits` /
`Double.doubleToRawLongBits`; no canonicalization, so the NaN payload and sign are hashed), and
`-0.0` differs from `0.0`. MineSim must therefore reproduce Java's NaN results exactly where Java
produces them (verify in `ms-numerics`).

---

## 4. Determinism obligations (why the hash can even be stable)

The hash is only meaningful if both vanilla (under the Step-5 harness) and MineSim are
deterministic. These obligations are enforced by the kernel, not the hash:

- **Iteration order:** where vanilla relies on fastutil `Long2ObjectOpenHashMap` /
  `Int2ObjectOpenHashMap` visitation order (entities, block entities, scheduled ticks),
  MineSim must reproduce **that** order exactly. A "stable insertion-ordered map" is the
  *wrong* order. This is a named work item in Step 6.
- **RNG draw order:** every consumer of a shared generator (`level.random` especially) must
  match vanilla's draw **count and order**. One extra or missing draw desyncs the run.
- **float→double promotion sites:** Java promotes/narrows at specific expression boundaries.
  Each site is reproduced by hand against the decompiled reference; codegen does not cover
  these.
- **No `target-cpu=native`, no fast-math, no auto-FMA, no system-libm transcendentals** in
  determinism-path crates. Numeric results must be identical on x86-64 **and** ARM64.

---

## 5. The freeze rule (= the release rule)

A kernel layer is **frozen** only after it replays hash-identical across its *entire* corpus
for that layer. On freeze:

1. The layer's corpus + golden hashes are committed.
2. CI gains a lock: any later change that perturbs a frozen layer's hash fails the build.

Because the accuracy posture is **strict bit-exact only**, "frozen" and "shippable" are the
same threshold. A subsystem that is not frozen is not part of any release; it is listed in
the conformance report as *deferred*, never as *approximate*.

**Mechanics.** The lock is `corpus/golden-hashes.json`, checked by `cargo test`
(`crates/ms-corpus/tests/golden.rs`, which CI runs on every push). Per corpus scenario it records the
rolling hash of the *recorded* states (a fingerprint of the corpus file and of the §2.0
serialization; it must always match), the rolling hash of the states the kernel produced when
blessed, and a `frozen` flag (kernel hash == recorded hash, i.e. the scenario was bit-exact when
blessed). A frozen scenario that no longer replays hash-identical fails the build, naming the
first diverging tick and field. `cargo xtask oracle --bless` rewrites the file; a scenario is
frozen by blessing it while it is exact, and a change that perturbs a frozen scenario must either be
fixed or deliberately re-blessed in review. Once every scenario has a blessed hash the file is
`locked` and a corpus scenario without an entry also fails. Details: `docs/corpus.md`.

---

## 6. Versioning

This contract is tagged (`contract-v1`, …). Any change to §2 (field set, widths, order),
§3 (hash function, seed, or the rolling-hash fold), or the NaN/`-0.0` handling requires a new tag
and a regeneration of all golden hashes (`cargo xtask oracle --bless`; the golden file records the
contract version and seed it was written for, and the lock fails on a mismatch rather than
comparing across versions). Targeting a new Minecraft version does **not** by itself
bump the contract version unless the state shape changes.
