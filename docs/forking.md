# Forking MineSim for another Minecraft version

MineSim targets one version at a time (currently 1.21.11). Retargeting is deliberately a
fork-and-regenerate exercise rather than a runtime option: the generated tables, the reference
traces, and the hand-written physics are all pinned to a single version's behavior, and mixing
versions would quietly break bit-exactness.

The oracle makes a fork *tractable and verifiable*, not *automatic*. It detects, per scenario,
the first tick and field where the new version diverges from the old behavior — which turns "port
MineSim to version X" into a ranked list of concrete behavior changes to work through. The Rust
edits themselves are written by hand.

## The workflow

1. **Pin the version.** Record version X in `docs/version-pin.json` (manifest hashes included).
   Obtain the official Mojang mappings; for a decompiled dev reference, run Vineflower with those
   mappings into `tools/decomp/` (git-ignored — see `docs/clean-room-policy.md`; never commit or
   redistribute decompiled sources or game jars).

2. **Regenerate data.** Run the server's `--reports` datagen for X, recompile the block-extraction
   mod against X, and run `cargo xtask regen-data`. This rewrites `ms-data`'s generated tables
   (block/state registry, collision shapes, friction). Diff the regenerated tables against the
   previous version to see what blocks and shapes changed.

3. **Regenerate the golden traces.** Recompile the trace mod against X and record the input corpus
   on vanilla X. These traces are the new ground truth.

4. **Run the differential sweep.** Replay the corpus through MineSim (still carrying the previous
   version's semantics) against the new traces. The oracle reports the first divergent field and
   tick per scenario — the ranked TODO list of behavior changes.

5. **Fix divergences layer by layer.** Work the list from the lowest layer up (numerics, then RNG,
   then collision, then movement), re-freezing each layer once it replays hash-identical again.

6. **Re-freeze and lock.** Once every layer is green against the new corpus, the version is frozen;
   CI replays the committed golden hashes so later changes cannot silently regress it.

## What changes between versions

Most version bumps touch the data, not the physics: new blocks, changed collision shapes, adjusted
attribute defaults. Those flow through step 2 with no code changes. Physics edits are rarer and the
oracle points straight at them — a changed drag constant or a reordered velocity update shows up as
a divergence at a specific tick.

Versions 26.1 and later are reported to ship without obfuscation, which removes the remapping step
entirely: a fork there is "rerun datagen, rerun the oracle, fix the diff list."

## Scope of a fork

A fork inherits MineSim's scope. Player movement and collision on static terrain are the validated
core; combat, projectiles, fluids, and the rest are added the same way they are in the base project
— behavior first, then a corpus, then a freeze — and only ship once they replay bit-for-bit.
