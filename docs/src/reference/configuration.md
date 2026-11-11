# Configuration

## Run-time environment

| Variable | Meaning |
|---|---|
| `LD_PRELOAD` | the tapir wrapper libraries to load (`:`-separated) |
| `TAPIR_PROFILERS` | profilers and options, e.g. `bbv(weight=time, lib=pix), summary, trace(fn=pix_*)`; the wrapper library chooses the default |
| `TAPIR_KEY` | extra key rules for every profiler, applied after the library's defaults |
| `TAPIR_OUT` | output path template; placeholders `{stream} {tag} {rank} {pid} {exe} {ext}`; default `tapir-out/{stream}.{tag}.{ext}` |
| `TAPIR_FORMAT` | default row format: `csv`, `tsv` or `jsonl` (also `json`, `ndjson`) |
| `TAPIR_START` | `paused` (or `0`) starts with recording off, until `tapir_enable(1)` |
| `TAPIR_VERBOSE` | `1` lists profilers at start and files written at exit, on stderr |
| `TAPIR_RANK` | force the rank used in file names |
| `TAPIR_GPU_SYNC` | `tapir-rocm` / `tapir-cuda` only: `device` synchronizes after each call |

Rank detection order: `TAPIR_RANK`, `OMPI_COMM_WORLD_RANK`, `PMI_RANK`,
`PMIX_RANK`, `SLURM_PROCID`, `MV2_COMM_WORLD_RANK`, `ALPS_APP_PE`.

## `tapir-run`

```text
tapir-run [-p PROFILERS] [-k RULES] [-o OUT] [-l LIB.so]... [-v] [--] command [args...]
```

| Flag | Sets |
|---|---|
| `-p` | `TAPIR_PROFILERS` |
| `-k` | `TAPIR_KEY` |
| `-o` | `TAPIR_OUT` |
| `-l` | a library to preload (repeatable); default `$TAPIR_LIB` (`:`-separated), else `target/release/libtapir_rocm.so`, else `libtapir_cuda.so` |
| `-v` | `TAPIR_VERBOSE=1` |

Existing `LD_PRELOAD` entries are kept, after the tapir libraries.

## `TAPIR_PROFILERS` syntax

```text
name[(key=value, key=value, ...)] [, name(...)]...
```

Lists inside a value use `;` (`fn=pix_load;pix_save`). `key` alone means
`key=` (true for flags).

## Profiler options

| Profiler | Option | Values | Default |
|---|---|---|---|
| all | `as` | instance name | the profiler's name |
| all | `format` | `csv`, `tsv`, `jsonl` | `TAPIR_FORMAT`, else `csv` |
| built-ins | `lib` | `a;b` | all libraries |
| built-ins | `fn` | `glob;glob` | all functions |
| built-ins | `nested` | `0`, `1` | `bbv`: 0; `summary`, `trace`: 1 |
| `bbv` | `key` | `exact`, `name`, or rules added to the defaults | library default + `TAPIR_KEY` |
| `bbv` | `weight` | `calls`, `time` | `calls` |
| `bbv` | `every` | `calls:N`, `time:SECONDS` | intervals closed by the application |
| `bbv` | `format` | `simpoint` (`bb`), `csv`, `tsv`, `jsonl` | `simpoint` |
| `summary` | `key` | `exact`, `default`, `name`, or rules on top of exact | `exact` |
| `summary` | `every` | `N`: partial dump every *N* intervals | 0 (off) |
| `trace` | `batch` | rows per write | 4096 |

## Key rules

```text
selector:bucket [; selector:bucket]...
```

| Selector | | Bucket | |
|---|---|---|---|
| `*` | every argument | `exact` | keep the value |
| `@tag` | arguments with this tag | `log2` | bit length |
| `@` | untagged arguments | `log2/N` | bit length, 2^N sub-buckets |
| `name` | argument named `name` | `sig/N` | *N* significant bits |
| `fn.name`, `fn.@tag`, `fn.*` | within functions matching glob `fn` | `linear/W` | bins of width *W* |
| | | `zeroone` | 0, 1, -1, other |
| | | `drop` | ignore |

Precedence: function-qualified > unqualified; then name > tag > `*`;
then the later rule. See [Block keys and bucketing](../guides/keys.md).

## Build-time environment (GPU libraries)

| Variable | `tapir-rocm` | `tapir-cuda` |
|---|---|---|
| Toolkit root | `ROCM_PATH`, `ROCM_HOME`, `CRAY_ROCM_DIR`, `HIP_PATH/..`; default `/opt/rocm` or newest `/opt/rocm-*` | `CUDA_HOME`, `CUDA_PATH`, `CUDA_ROOT`, `CUDATOOLKIT_HOME`, `NVHPC_CUDA_HOME`; default `/usr/local/cuda` |
| Extra include dirs | | `TAPIR_CUDA_INCLUDE_DIRS=a:b` |
| Libraries | `TAPIR_ROCM_LIBS=rocblas,rocsolver,hipblas,hipsolver,rocsparse` | `TAPIR_CUDA_LIBS=cublas,cusolver,cusparse` or `none` |
| Name globs | `TAPIR_ROCM_INCLUDE`, `TAPIR_ROCM_EXCLUDE` | `TAPIR_CUDA_INCLUDE`, `TAPIR_CUDA_EXCLUDE` |

## The `Builder` (inside `preload!` / `tapir::init`)

| Method | Effect |
|---|---|
| `from_env(default)` | enable `TAPIR_PROFILERS`, or `default` if unset |
| `enable(Opts)` | enable a named profiler with options |
| `factory(name, f)` | register a profiler constructor `Fn(&Opts) -> Result<P, String>` |
| `add(profiler)` | add a profiler instance unconditionally |
| `default_key(rules)` | append to the library's default key rules |
| `output(template)` | output template, overriding `TAPIR_OUT` |
| `after_call(fn())` | hook run after each recorded call, inside the timed region |
| `paused()` | start with recording off |
