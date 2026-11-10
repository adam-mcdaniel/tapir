# Nuggets: CPU + GPU simulation points with tapir

`analysis.c` is the Nuggets analysis hook with GPU library-call vectors added.
At each CPU region boundary it takes the `bbv` profiler's current interval
with `tapir_try_vector` and writes it next to the CPU rows of the same region.

1. Copy `analysis.c` and `../../include/tapir.h` into the app's
   `toolchain/Nugget/hooks/` (replacing `analysis.c`), and build the analysis
   binary as usual. The hook needs nothing at link time. Without tapir it
   behaves like the CPU-only original.
2. Run the analysis binary under tapir with the ROCm wrappers:
   ```sh
   srun -A csc688 -N1 -n1 --gpus-per-task=1 \
       tapir/bin/tapir-run -p 'bbv' -- ./build/llvm-exec/app-analysis-exec ...
   ```
   On an NVIDIA machine, run the CUDA build of the same analysis binary under
   the CUDA wrappers instead; the hook and its output are the same:
   ```sh
   tapir/bin/tapir-run -l tapir/target/release/libtapir_cuda.so -p 'bbv' -- ./app-analysis-exec ...
   ```
3. Open `simpoint_analysis.ipynb` unchanged. Per region the hook writes
   `gpu_bb_id,<region>,<hash>...` and `gpu_bbv,<region>,<calls>...` into
   `analysis-output.csv` (path: `NUGGET_ANALYSIS_OUTPUT`; profiler sampled:
   `NUGGET_GPU_PROFILER`, default `bbv`).

GPU "basic blocks" come from `libtapir_rocm.so` (or `libtapir_cuda.so`), whose parameters are tagged
with the `linalg` vocabulary (`crates/tapir-vocab`): sizes (`@dim`) and counts
(`@count`) are bucketed by bit length, modes and strides kept exact, scalars
and pointers ignored. To match the notebook's `GPU_DIM_BITS=3`, use
`-p 'bbv(key=@dim:sig/3)'`. `sig/3` reproduces the notebook's `_relbucket(v, 3)`
exactly. To keep bucketing tunable without re-running, record with
`bbv(key=exact)` and re-bucket offline with `python/tapir_read.py`
(`rebucket(blocks, dim=lambda v: relbucket(v, 3))`).

Compared with `perf-libs-tools` plus the notebook:

| | before | tapir |
|---|---|---|
| GPU "basic block" | FNV hash of the full signature, re-bucketed in Python (`_relbucket` on every int, which merges `transA=111/112`, side, fill and diag) | Bucketed online by tag: sizes `log2` (or `sig/3`), modes exact, pointers/scalars dropped |
| Region alignment | interceptor's private dump counter | hook's region index |
| Nested calls (rocSOLVER → rocBLAS) | counted twice | top-level only (`nested=1` to include) |
| Block ids | per-signature hash | stable FNV-1a hash of the bucketed label, identical across ranks and runs |
| Wrapper generation | Python regex over prototype lists → 76k-line C file, varargs | `tapir-gen` parses headers → typed Rust wrappers, no varargs |
| Per-call cost | `dlsym` + string building + list search | ~110 ns for `bbv` (~25 ns with recording paused) |
| Copies | one per app directory, diverged | one crate; libraries chain if several are preloaded |

`test/run.sh` checks the whole path end to end on a login node: a fake GPU
library (`examples/fakeblas`), tapir, the hook, and the notebook's own parser.
