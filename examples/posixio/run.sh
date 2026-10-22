#!/usr/bin/env bash
# Build the posixio example and check its output: tapir with a library that
# has nothing to do with numerics.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:-$(mktemp -d)}
mkdir -p "$out"
cargo build --release -q --manifest-path "$root/Cargo.toml" -p tapir-posixio
# -rdynamic exports cmp_up so `sym` can name it; static cmp_down stays app+offset.
gcc -O1 -rdynamic -o "$out/app" "$here/c/app.c"
TAPIR_PROFILERS='bbv,summary(key=default),trace' TAPIR_OUT="$out/tapir/{stream}.{ext}" \
    LD_PRELOAD="$root/target/release/libtapir_posixio.so" "$out/app" "$out"
python3 "$here/check.py" "$out/tapir"
