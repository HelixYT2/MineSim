# MineSim Bit-Exact Contract

**Status:** `contract-v1` (draft — freeze and tag before any kernel work begins)
**Target:** Minecraft Java Edition **1.21.11** (Java 21 runtime semantics)

This document is the **authoritative definition of correctness** for MineSim. Every
acceptance gate is stated in terms of this contract. It is
deliberately mechanical: correctness is byte equality, not human judgement.

---

## 1. Definition

A MineSim run is **bit-exact** with vanilla over an input corpus *iff*, for every tick
`t ∈ [0, N)`, the canonical per-tick **state hash** `H(t)` computed by MineSim equals the
hash dumped by the oracle trace mod for the **same** scripted inputs, world seed, and
gamerule/configuration — for all `t`.

There is **no epsilon and no ULP tolerance.** A single differing bit at any tick is a
failure. This is a deliberate, project-wide decision: Minecraft state is recursive across
ticks (`pos(t+1)` is a function of `pos(t)`), so any rounding difference, however small,
will eventually cross a collision boundary or a comparison branch and fork the trajectory.

A subsystem that cannot (yet) be made bit-exact is **deferred**, not approximated. It does
not ship until it is provable.

---

## 2. Canonical state and its serialization

The byte layout below **is** the contract. Both sides (the Java oracle trace mod and the
Rust `ms-oracle`/`ms-arena` hasher) must produce identical bytes for identical state.
Freeze this layout in Step 1; changes require a new contract version tag.

All multi-byte integers are **big-endian**. Field order is fixed (see §3).

| Field class | Source type (Java) | Serialization |
|---|---|---|
| Position `x,y,z` | `double` | `Double.doubleToRawLongBits` → `u64` BE (Vec3 = 3×u64) |
| Velocity `dx,dy,dz` | `double` | raw `u64` BE |
| `fallDistance` | `double`¹ | raw bits (width matches the game field — verify per version) |
| Rotation `yaw,pitch` | `float` | `Float.floatToRawIntBits` → `u32` BE |
| Any `float` game field | `float` | raw `u32` BE — **never** promote to f64 before hashing |
| Movement inputs `xxa,yya,zza` | `float` | raw `u32` BE |
| Float attribute values | `float`/`double` (per attr) | raw bits at the field's true width |
| Booleans/flags² | `boolean` | `u8` (`0x00`/`0x01`) |
| Integer fields | `int`/`long` | wrapping two's-complement, raw `i32`/`i64` BE |

¹ `fallDistance` width changed across versions; confirm against the decompiled reference
for 1.21.11 and record the chosen width here.
² Minimum flag set: `onGround`, `horizontalCollision`, `verticalCollision`, `isSprinting`,
`isSwimming`, `isInWater`, `isInLava`, `noPhysics`. Extend as kernel layers add state.

### 2.1 RNG state (the earliest divergence detector)

RNG drift desynchronizes everything downstream and is usually the *first* thing to diverge,
so it is part of the hash:

- **`java.util.Random` (LCG):** the 48-bit seed as `u64` BE, plus `haveNextNextGaussian`
  (`u8`) and `nextNextGaussian` (raw `u64` bits) when the generator is a `java.util.Random`.
- **Xoroshiro128++ (`RandomSource`):** the two 64-bit state words `seedLo`, `seedHi` as
  `u64` BE each.

Every generator instance that influences hashed state (entity RNG, `level.random`, etc.)
contributes its state in a fixed slot.

### 2.2 Block deltas

The **ordered** list of block changes applied during the tick:
`(BlockPos packed as i64 BE, prior_state_id u32 BE, new_state_id u32 BE)`, in **application
order**. Order is part of the contract — it encodes the neighbor-update direction orders
(Place/Physics `W,E,N,S,D,U`; neighbor-changed `W,E,D,U,N,S`; comparator `N,E,S,W`).

### 2.3 Entity set membership

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

- `serialize` concatenates the §2 fields in a **frozen field order** (documented alongside
  the canonical-state struct in `crates/*` and `tools/trace-mod`). Both implementations
  must agree field-for-field.
- `MINESIM_HASH_SEED` is a fixed constant, chosen once, recorded here, and version-stamped.
- xxh3 is chosen for speed; it is **not** cryptographic and that is fine — the hash only
  needs to be a stable, collision-resistant-enough fingerprint for equality checks.

**Two comparison modes:**
1. **Hash equality** — fast path for corpus sweeps. `H_minesim(t) == H_oracle(t)` for all `t`.
2. **Field-level first-divergence diff** — debug path. Both sides re-dump the raw §2 fields
   (not just the hash); the harness reports the earliest tick and the first differing field.
   This is the primary bisection tool when a gate fails.

NaN is compared **by bit pattern** (raw bits), so MineSim must reproduce Java's NaN
canonicalization exactly where Java canonicalizes (verify in Step 2 / `ms-numerics`).

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

---

## 6. Versioning

This contract is tagged (`contract-v1`, …). Any change to §2 (field set, widths, order),
§3 (hash function or seed), or the canonical NaN handling requires a new tag and a
regeneration of all golden hashes. Targeting a new Minecraft version does **not** by itself
bump the contract version unless the state shape changes.
