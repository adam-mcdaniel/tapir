#!/usr/bin/env bash
# End-to-end check of the Nuggets integration without GPUs or LLVM:
# fake library + tapir preload + the analysis hook + the notebook's parser.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
out=${1:-$(mktemp -d)}
mkdir -p "$out"
cargo build --release -q --manifest-path "$root/Cargo.toml" -p tapir-fakeblas
gcc -O1 -shared -fPIC -o "$out/libfakeblas.so" "$root/examples/fakeblas/c/fakeblas.c"
cp "$root/include/tapir.h" "$out/"
gcc -O0 -I"$out" -o "$out/driver" "$here/driver.c" "$here/../analysis.c" -L"$out" -lfakeblas -Wl,-rpath,"$out"
cd "$out"
TAPIR_PROFILERS=bbv TAPIR_OUT="$out/tapir/{stream}.{tag}.{ext}" \
    LD_PRELOAD="$root/target/release/libtapir_fakeblas.so" ./driver
python3 "$here/check.py" "$out/analysis-output.csv"
