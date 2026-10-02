# refgen: reference vectors from the real JVM and the real game

The numeric and RNG ports (`crates/ms-numerics`, `crates/ms-rng`) are checked bit-for-bit against
data produced by two small Java generators. Their output is committed under each crate's
`testdata/` (and `data/` for the embedded lookup tables), so ordinary `cargo test` needs no JVM.

| Generator | Needs | Writes |
| --- | --- | --- |
| `RefGen.java` | a JDK 21 only | `ms-rng/testdata/java_random.csv`, `ms-numerics/testdata/fdlibm_reference.csv`, `ms-numerics/testdata/hotspot_log.csv` |
| `GameGen.java` | the game jar + its libraries | `ms-numerics/data/mth_sin_table.bin`, `ms-numerics/data/mth_atan_tables.bin`, `ms-numerics/testdata/mth_reference.csv`, `ms-rng/testdata/{legacy_random,xoroshiro_random,random_support}.csv` |

## Regenerating

```sh
# JVM-only vectors
tools/refgen/regen.sh

# plus the vectors that call the game's own classes
MC_JAR=/path/to/minecraft-common-...-1.21.11....jar \
MC_LIBS=/path/to/extracted/META-INF/libraries \
  tools/refgen/regen.sh
```

`MC_JAR` is a mojmap-named 1.21.11 game jar (the Loom cache of `tools/minesim-mod` has one:
`.gradle/loom-cache/minecraftMaven/net/minecraft/minecraft-common-*/.../minecraft-common-*.jar`).
`MC_LIBS` is any directory tree holding the game's library jars (guava, joml, commons-lang3, ...),
for example the `META-INF/libraries` folder of the server jar after `unzip`. **Never commit a game
jar, a decompiled source tree or the extracted libraries.**

The generators are deterministic (fixed seeds): regenerating on the same JVM reproduces the
committed files byte for byte.

## File formats

All numbers are raw bits so comparisons are exact: floats as `Float.floatToRawIntBits`, doubles as
`Double.doubleToRawLongBits`, printed as unsigned decimal. Rust tests treat "NaN equals NaN"
(NaN payloads are not part of the contract), everything else must match bit for bit.

* `mth_reference.csv`: `op,a,b,c,result`. One row per call of a real `net.minecraft.util.Mth`
  method (`sin_f`/`cos_f` feed a widened `float`, `sin_d`/`cos_d` a `double`, `floor_d`,
  `lfloor`, `clamp_f`, `wrap_f`, `atan2`, `get_seed`, ...) or of `java.lang.Math.min/max`.
* `fdlibm_reference.csv`: `fn,a_bits,b_bits,result_bits` for `StrictMath.acos/atan/atan2/log`
  (`atan2` takes `a = y`, `b = x`). The generator asserts that `Math.acos/atan/atan2` equal the
  `StrictMath` results on every input (on this JDK they delegate to fdlibm).
* `hotspot_log.csv`: `a_bits,math_log_bits,correctly_rounded_bits`: `Math.log` as the JVM executed
  it next to the correctly rounded logarithm (80-digit `BigDecimal`).
* `java_random.csv`: `seed,op,arg,bits` for `java.util.Random` (includes `nextGaussian`).
* `legacy_random.csv`, `xoroshiro_random.csv`: `op,a,b,c,s,bits`. Each sequence starts with `new`
  (or `new128`) and mixes every draw kind (`nextInt`, bounded, inclusive range, `nextLong`,
  boolean, float, double, gaussian, `triangle`, `consumeCount`, `fork`, `forkPositional` with
  `at`/`fromHashOf`/`fromSeed`, `setSeed`). `s` is a UTF-8 string argument in hex.
* `random_support.csv`: `RandomSupport.mixStafford13`, `upgradeSeedTo128bit(Unmixed)`,
  `seedFromHashOf` (MD5), and `String.hashCode`.

## Platform notes recorded by the generators

* `Math.log` is a HotSpot intrinsic (Intel LIBM stub on x86_64) and differs from
  `StrictMath.log` (fdlibm) on about 7% of arguments; the stub is correctly rounded except for
  roughly 1 argument in 80 000 (619 of 50 million mixed samples). The game's `nextGaussian` calls `Math.log`.
  `ms_numerics::hotspot::log` reproduces it as the correctly rounded value;
  `ms_numerics::fdlibm::log` is the exact `StrictMath.log`.
* `Mth.SIN` agrees entry for entry with `StrictMath`; in `Mth.COS_TAB` (used by `Mth.atan2`)
  10 of 257 entries differ between HotSpot's `Math.cos` stub and `StrictMath.cos`. The embedded
  table is what the game builds on x86_64 HotSpot.
