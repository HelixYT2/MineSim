# MineSim probe mod

A Fabric mod for Minecraft 1.21.11 that extracts the data MineSim is generated from and records
the ground truth it is validated against. Nothing here ships with MineSim; it runs inside the real
game.

## What it does

- **Block data.** `/minesim dumpblocks` writes `minesim-blocks.json`: every block state's
  collision boxes, fluid and suffocation flag, and every block's class, friction, speed and jump
  factors and tags, all floats as raw IEEE-754 bits. `cargo xtask regen-data` turns it (with the
  vanilla `--reports` datagen) into the `ms-data` tables.
- **Scenario oracle.** With `-Dminesim.scenarios=all` the client drives the player through the
  scripted scenarios in `src/client/java/minesim/client/Scenarios.java` and writes one trace per
  scenario. This is the corpus in `corpus/`; `docs/corpus.md` describes the format.
- **Passive tracing.** Without that property, the mod records ordinary play: the client writes
  `minesim-trace.jsonl` (the local player each tick) and the server writes
  `minesim-trace-server.jsonl` (every living entity and projectile each tick).

## Recording the corpus

The `runOracle` task launches the client into the singleplayer world `run/saves/oracle`, runs
every scenario, and quits:

```
./gradlew runOracle                        # all scenarios
./gradlew runOracle -Pscenarios=walk_basic,water_pool
./gradlew runOracle -Pdumpblocks=true      # also write run/minesim-blocks.json
xvfb-run -a ./gradlew runOracle            # headless on Linux
```

The `oracle` world is a superflat world with a single stone layer at y = -64 and cheats enabled.
To make one, start the dedicated server once with `level-type=minecraft:flat` and
`generator-settings={"layers":[{"block":"minecraft:stone","height":1}],"biome":"minecraft:plains"}`
in `run/server.properties` (`./gradlew runServer`), copy `run/world` to `run/saves/oracle`, and
set `allowCommands` to 1 in its `level.dat`. Each scenario clears and rebuilds its own arena around
the origin, resets the player (health, food, effects, position), and switches off everything
nondeterministic (daylight, weather, random ticks, mob spawning, natural regeneration, movement
checks), so the world only needs to exist.

Then compress the traces into the repository:

```
for f in run/minesim-corpus/client/*.jsonl; do gzip -9 -n -c "$f" > ../../corpus/client/$(basename "$f").gz; done
```

## Adding a scenario

A scenario is a block layout, a start state, and an input program:

```java
Scenario.of("ladder_climb", "Climbing a ladder by walking into it ...")
    .fill(-2, Y0, 4, 2, Y0 + 6, 5, "minecraft:stone")
    .fill(0, Y0, 3, 0, Y0 + 6, 3, "minecraft:ladder[facing=north,waterlogged=false]")
    .start(0.5, Y0, -0.5, 0.0F, 0.0F)
    .hold(50, "w")          // keys: w a s d, j jump, c sneak, r sprint
    .hold(20, "wc")
    .now(Scenarios.hurt(2.0F, 0.5, 3.5));   // server-side action at this tick
```

Add it to `Scenarios.all()`, record it, and commit the trace.

## Building

Java 21 and the Gradle wrapper: `./gradlew build`. The access widener
(`src/main/resources/minesim.accesswidener`) exposes the private fields the traces log.
