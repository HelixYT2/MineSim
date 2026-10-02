#!/usr/bin/env bash
# Regenerates the committed reference vectors from a real JVM and the real game classes.
#
#   tools/refgen/regen.sh                 # JVM-only vectors (java.util.Random, fdlibm, Math.log)
#   MC_JAR=/path/minecraft-common-....jar MC_LIBS=/path/to/libraries tools/refgen/regen.sh
#                                         # ... plus Mth / RandomSource / RandomSupport vectors
#
# MC_JAR  a mojmap-named 1.21.11 game jar (the Loom cache's minecraft-common-*.jar works)
# MC_LIBS a directory tree of the game's library jars (extract META-INF/libraries from the
#         server jar); every *.jar below it goes on the classpath
#
# Needs a JDK 21 on PATH. The jars are read, never copied: do NOT commit them (or any
# decompiled source). See tools/refgen/README.md for what is produced and why.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

echo "== RefGen (JVM only)"
javac -proc:none -d "$out" "$root/tools/refgen/RefGen.java"
java -cp "$out" RefGen "$root"

if [[ -n "${MC_JAR:-}" && -n "${MC_LIBS:-}" ]]; then
  echo "== GameGen (real game classes)"
  libs="$(find "$MC_LIBS" -name '*.jar' | tr '\n' ':')"
  javac -proc:none -d "$out" -cp "$MC_JAR:$libs" "$root/tools/refgen/GameGen.java"
  java -cp "$out:$MC_JAR:$libs" GameGen "$root"
else
  echo "(skipping GameGen: set MC_JAR and MC_LIBS to regenerate the Mth/RandomSource vectors)"
fi
