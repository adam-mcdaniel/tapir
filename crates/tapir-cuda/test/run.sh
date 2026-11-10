#!/usr/bin/env bash
# GPU-free check of libtapir_cuda.so: a fake libcudart/libcublas, an app that
# calls it like a CUDA program, and the profile tapir writes. Needs gcc and
# cargo on Linux x86-64 or aarch64; no CUDA toolkit (the two cuBLAS prototypes
# come from test/include). The library is built into its own target directory,
# so the real target/release/libtapir_cuda.so is never replaced.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
out=${1:-$(mktemp -d)}
mkdir -p "$out"
env -u CUDA_HOME -u CUDA_PATH -u CUDA_ROOT TAPIR_CUDA_INCLUDE_DIRS="$here/include" TAPIR_CUDA_LIBS=cublas \
    cargo build --release -q --manifest-path "$root/Cargo.toml" -p tapir-cuda --target-dir "$out/target"
gcc -O1 -shared -fPIC -o "$out/libfakecuda.so" "$here/fakecuda.c"
# -rdynamic exports axpy_kernel so its launches are named; static local_kernel stays app+offset.
gcc -O1 -rdynamic -o "$out/app" "$here/app.c" -L"$out" -lfakecuda -Wl,-rpath,"$out"
TAPIR_PROFILERS='bbv,summary' TAPIR_OUT="$out/tapir/{stream}.{ext}" \
    LD_PRELOAD="$out/target/release/libtapir_cuda.so" "$out/app" > "$out/stdout.txt"
python3 "$here/check.py" "$out/tapir" "$out/stdout.txt"
