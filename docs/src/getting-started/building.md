# Building tapir

Clone the repository and build in release mode:

```sh
git clone <tapir-repo-url> tapir
cd tapir
cargo build --release
```

A plain `cargo build` builds the parts that need no GPU toolkit:

| Artifact | What it is |
|---|---|
| `target/release/libtapir.rlib` | the core crate, used by every wrapper library |
| `target/release/tapir-gen` | the header-to-wrapper generator (command-line form) |
| `target/release/libtapir_posixio.so` | example: hand-written wrappers for `fopen`, `fwrite`, `qsort`, ... |
| `target/release/libtapir_fakeblas.so` | example: generated wrappers for a fake BLAS, plus a custom profiler |

The GPU wrapper libraries are separate packages because they read the
vendor's headers at build time:

```sh
cargo build --release -p tapir-rocm   # AMD:    libtapir_rocm.so, needs ROCm headers (ROCM_PATH)
cargo build --release -p tapir-cuda   # NVIDIA: libtapir_cuda.so, needs CUDA headers (CUDA_HOME)
```

The [GPU case study](../guides/gpu-libraries.md) explains how they are
built and configured.

## Checking the build

```sh
cargo test                          # unit tests and the runtime integration test
examples/posixio/run.sh             # builds a C program, profiles it, checks the output
crates/tapir-cuda/test/run.sh       # tapir-cuda against a fake CUDA library (no GPU needed)
integrations/nuggets/test/run.sh    # the Nuggets hook end to end
```

The `run.sh` scripts need `gcc` and `python3`.

## Repository layout

```text
crates/tapir/        core: capture modes, interpose!, runtime, KeyPolicy/BlockTable, profilers, sinks, C ABI
crates/tapir-gen/    C header -> interpose! generator (library + CLI)
crates/tapir-vocab/  shared vocabularies (linalg)
crates/tapir-rocm/   libtapir_rocm.so: ROCm math libraries and the HIP runtime
crates/tapir-cuda/   libtapir_cuda.so: CUDA math libraries and the CUDA runtime
include/tapir.h      C ABI + runtime-resolved helpers
examples/posixio/    hand-written wrappers for stdio and qsort
examples/fakeblas/   generated wrappers for a GPU-free BLAS-like library + a custom profiler
integrations/nuggets Nuggets analysis hook with GPU vectors
python/tapir_read.py readers for blocks and .bb files; offline re-bucketing by tag
bin/tapir-run        launcher
docs/                this book
```

## Building this book

```sh
cargo install mdbook
mdbook serve docs      # http://localhost:3000
mdbook build docs      # static site in docs/book/
```

Diagrams are Graphviz files in `docs/src/diagrams/*.dot`. The rendered
SVGs are committed next to them, so building the book does not need
Graphviz. After editing a `.dot` file, run `docs/src/diagrams/render.sh`.
