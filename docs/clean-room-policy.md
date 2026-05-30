# MineSim Clean-Room Policy

MineSim reimplements the *behavior* of Minecraft Java Edition. It must not copy or
redistribute Mojang's code or assets. This policy defines the boundary that keeps the
project legally clean while still allowing us to validate bit-exactly against the real game.

## Principles

1. **Reimplement behavior, not source.** MineSim code is authored from observed behavior,
   public specifications (e.g. [minecraft.wiki](https://minecraft.wiki),
   [mcpk.wiki](https://www.mcpk.wiki)), and the project's own differential oracle — *not*
   by transcribing decompiled Mojang source.

2. **Decompiled sources are reference-only and never committed.** Vineflower + Mojmap +
   Parchment output (under `tools/decomp/`) is a developer aid for *understanding semantics*
   and for locating the precise float→double promotion sites and operation order. It is:
   - **git-ignored** (never committed to the repo),
   - **never redistributed** in any form (source, comments, or pasted snippets),
   - used to *check* an independent reimplementation, not to seed it.

3. **No Mojang artifacts in the repo.** Do not commit Minecraft client/server JARs,
   obfuscation maps, datagen output containing Mojang text, or decompiled source. CI fetches
   the vanilla server JAR on demand, pinned by hash, and treats it as an external dependency
   under Mojang's terms — it is never republished.

4. **The oracle is the proof, not the source.** Correctness is demonstrated by the trace
   mod + replay harness (byte-identical output), which requires *behavioral* equivalence and
   does not require us to ship any Mojang code.

5. **Contributor rule.** Contributions must not contain code copied or mechanically
   translated from decompiled Minecraft. By submitting, contributors affirm their code is an
   independent reimplementation. PRs that paste Mojang source (even as comments) are rejected.

## What is OK

- Reading decompiled 1.21.11 locally to learn the exact operation order, constants, and
  promotion sites, then writing original Rust that produces the same observable result.
- Generated data tables derived from Minecraft's own `--reports` datagen (block-state IDs,
  registries) and from CC0-licensed Parchment param names — treated as data, regenerated per
  version, with provenance documented.
- Citing Mojang class/method names (e.g. `LivingEntity.travel`) in comments and docs to
  orient readers; names are not the copyrighted expression.

## What is NOT OK

- Committing or redistributing decompiled Mojang source or Minecraft JARs/assets.
- Pasting decompiled method bodies into MineSim (in code or comments).
- Shipping Mojang's obfuscation maps or wholesale datagen text dumps.

> This is an engineering policy, not legal advice. If the project moves toward a formal
> public release, obtain a proper legal review of the clean-room boundary and Mojang's EULA.
