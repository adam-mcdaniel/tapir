# tapir — Tracing And Profiling Interposition Runtime

General-purpose Rust infrastructure for wrapping C functions, capturing their
arguments, and feeding the calls to modular profilers that are sampled
**online** (by your own code, at your own boundaries) or dumped **offline** at
exit. The core knows nothing about any library: what an argument *means* is a
free-form tag chosen by whoever writes the wrapper.

```
app ──calls──▶ fwrite / rocblas_dgemm / ... (tapir wrapper) ──dlsym(RTLD_NEXT)──▶ real library
                     │ arguments captured by mode, labelled by tag:
                     │   size=val@bytes  path=cstr@path  compar=sym  buf=skip
                     ▼
               profilers: bbv · summary · trace · <yours>
                     │                     ▲
                     ▼                     │ tapir_interval / tapir_vector /
           tapir-out/{stream}.{tag}.{ext}  │ tapir_snapshot / tapir_record (C ABI)
                                    online drivers, other tools
```

Zero runtime dependencies. Three layers:

| Layer | Crate | Knows about |
|---|---|---|
| Core | `crates/tapir` | C calling conventions, capture modes, tags (as opaque strings), intervals, profilers, output |
| Vocabulary | `crates/tapir-vocab` | one API family: which parameter names get which tags, and how to bucket them |
| Wrapper library | `crates/tapir-rocm`, `crates/tapir-cuda`, `examples/*` | one concrete library: its headers, its types |

## Quick start

```sh
cd tapir
cargo build --release                 # core, generator, examples: no GPU toolkit needed
cargo build --release -p tapir-rocm   # AMD:    libtapir_rocm.so, needs ROCm headers (ROCM_PATH)
cargo build --release -p tapir-cuda   # NVIDIA: libtapir_cuda.so, needs CUDA headers (CUDA_HOME)

# Profile the GPU math libraries and runtime calls of whichever was built
# (rocBLAS, rocSOLVER, hipBLAS, hipSOLVER, rocSPARSE, HIP; or cuBLAS, cuSOLVER, cuSPARSE, CUDA):
bin/tapir-run -v -p bbv,summary -- ./app
# A different wrapper library, here the stdio example:
bin/tapir-run -l target/release/libtapir_posixio.so -p summary,trace -- ./app
ls tapir-out/
#   bbv.r0.bb            one SimPoint vector per interval
#   bbv.blocks.r0.csv    block id/hash -> function + bucketed args
#   summary.r0.csv       calls and time per exact signature
```

`tapir-run` only sets `LD_PRELOAD` and a few environment variables:
`TAPIR_PROFILERS=bbv LD_PRELOAD=$PWD/target/release/libtapir_rocm.so ./app`
is equivalent (`libtapir_cuda.so` on an NVIDIA machine).

## Either vendor

`libtapir_rocm.so` and `libtapir_cuda.so` are the same tool for the two GPU
stacks, so a profile can be taken on whichever machine is at hand:

| | `tapir-rocm` (AMD) | `tapir-cuda` (NVIDIA) |
|---|---|---|
| Math libraries, generated from the installed headers | rocBLAS, rocSOLVER, hipBLAS, hipSOLVER, rocSPARSE | cuBLAS, cuSOLVER (Dn, Sp, Rf), cuSPARSE |
| Runtime: allocation and free, copies, memsets | `hipMalloc`, `hipMemcpy`, ... (`src/hip.rs`) | `cudaMalloc`, `cudaMemcpy`, ... (`src/cudart.rs`) |
| Kernel launches, as kernel + blocks + threads | `hipLaunchKernel`, ... | `cudaLaunchKernel`, `cudaLaunchKernelExC`, ... |
| Wait for the GPU after each call | `TAPIR_GPU_SYNC=device` | `TAPIR_GPU_SYNC=device` |

Both tag arguments with the `linalg` vocabulary and use its key rules, and the
profilers, the output files and the C ABI are the core's, so everything
downstream of a profile (the Nuggets hook, `python/tapir_read.py`) reads
either one unchanged. The block names and hashes are each vendor's own
(`rocsolver_zgetrf` on one side, `cusolverDnZgetrf` on the other): a profile
describes the run on the machine it was taken on, and blocks are not matched
across vendors.

Integrations: [Nuggets CPU + GPU simulation points](integrations/nuggets/README.md).

## Concepts

**Capture modes** say *how* an argument is read. They are mechanical and the
same for every C library; the compiler rejects a mode that does not fit the
type.

| mode | reads | e.g. |
|---|---|---|
| `val` | the argument itself (pointers as their address) | `int n`, `size_t len`, enums |
| `deref` | the scalar behind a host pointer | Fortran `const int *n` |
| `chr` / `deref_chr` | a `char`, or the `char` behind a pointer | `char uplo`, Fortran `const char *job` |
| `cstr` | a NUL-terminated string | `const char *path` |
| `sym` | a code address, reported by symbol name (`qsort`'s `compar`) | callbacks |
| `skip` | nothing | handles, data buffers |

`deref`, `deref_chr` and `cstr` read memory: only use them where the pointer is
readable host memory (a GPU library's `alpha` may be a device pointer).
`sym` names functions through `dladdr`: exported symbols by name, others as
`object+offset` (stable across runs; link executables with `-rdynamic` to get
names).

**Tags** say what an argument *means*: any string (`"bytes"`, `"path"`,
`"rank"`, `"dim"`). tapir never interprets them; profilers and key rules
select on them (`c.tagged("bytes")`, `@bytes:log2`). Untagged arguments are
fine.

**Keys (bucketing)** decide which calls count as the same block, for `bbv`
and `summary(key=...)`. Rules are `selector:bucket`, joined with `;`:

```
@bytes:log2               arguments tagged "bytes", by bit length
@size:sig/3               3 significant bits
n:exact                   the argument named n, in any function
my_*.flags:drop           argument flags of functions matching my_*
my_fn.@size:log2/2        tag within matching functions, 4 sub-buckets per octave
@:drop                    untagged arguments
*:drop                    everything not matched by a more specific rule
```

The most specific rule wins (function-qualified over not; name over tag over
`*`); among equals, the last. Without any rule an argument is kept exactly.
Buckets: `exact`, `log2`, `log2/N`, `sig/N`, `linear/W`, `zeroone`, `drop`.
Data pointers never enter a key (their addresses change between runs);
strings and symbols are always exact (strings are percent-escaped in labels).
Rules come from three places, applied in order: the wrapper library's
default (`b.default_key(...)`), `TAPIR_KEY`, and the profiler's `key=` option.

**Intervals.** Profilers accumulate per interval. An interval is closed:
- *online*, by an external driver: `tapir_interval(i)`, or `tapir_vector(..., take=1, ...)`;
- by the profiler itself: `bbv(every=calls:1000)`, `bbv(every=time:0.5)`;
- or never, in which case the whole run is one interval written at exit.

**Output.** Each profiler writes *streams* through a sink. The default path is
`tapir-out/{stream}.{tag}.{ext}`, where `tag` is `r<rank>` under MPI (rank from
`OMPI_COMM_WORLD_RANK`/`PMI_RANK`/`SLURM_PROCID`/…) and `p<pid>` otherwise.
Existing files are never overwritten; a `.1`, `.2`, … suffix is added instead.
A process that never sees an event (a shell or helper that inherited
`LD_PRELOAD`) writes nothing. Row formats are `csv`, `tsv` and `jsonl`; `bbv`
also writes SimPoint `.bb`.

## Configuration

| Variable | Meaning |
|---|---|
| `TAPIR_PROFILERS` | e.g. `bbv(weight=time,lib=stdio), summary, trace(fn=f*)`; the wrapper library sets the default. `name(as=alias,...)` runs a second instance. |
| `TAPIR_KEY` | extra key rules for every profiler, applied after the library's defaults |
| `TAPIR_OUT` | path template; placeholders `{stream} {tag} {rank} {pid} {exe} {ext}` |
| `TAPIR_FORMAT` | default row format (`csv`/`tsv`/`jsonl`) |
| `TAPIR_START=paused` | start paused; `tapir_enable(1)` to begin (e.g. at a region of interest) |
| `TAPIR_VERBOSE=1` | list profilers and written files on stderr |
| `TAPIR_RANK` | force the rank used in file names |

Profiler options:

| Profiler | Options |
|---|---|
| `bbv` | `key=` (rules added to the defaults, or `exact`, `name`), `weight=calls\|time`, `every=calls:N\|time:S`, `format=simpoint\|csv\|tsv\|jsonl` |
| `summary` | `key=exact\|default\|name\|<rules>` (default `exact`), `every=N` (partial dump every N intervals), `format=` |
| `trace` | `batch=N`, `format=` |
| all | `lib=a;b`, `fn=glob;glob`, `nested=0\|1`, `as=name` |

`tapir-rocm` and `tapir-cuda` add `TAPIR_GPU_SYNC=device` (wait for the GPU
after each call, so durations cover execution, not just the asynchronous
launch). At build time:

| | `tapir-rocm` | `tapir-cuda` |
|---|---|---|
| Toolkit root | `ROCM_PATH` | `CUDA_HOME` (or `CUDA_PATH`); `TAPIR_CUDA_INCLUDE_DIRS=a:b` adds include directories |
| Libraries to wrap | `TAPIR_ROCM_LIBS=rocblas,rocsolver,hipblas,hipsolver,rocsparse` | `TAPIR_CUDA_LIBS=cublas,cusolver,cusparse` (`none`: runtime calls only) |
| Name globs, `;`-separated | `TAPIR_ROCM_INCLUDE` / `TAPIR_ROCM_EXCLUDE` | `TAPIR_CUDA_INCLUDE` / `TAPIR_CUDA_EXCLUDE` |

Setup and query APIs (`*_create*`, `*_set_*`, `*_bufferSize`…) are excluded by
default. An NVIDIA HPC SDK's `math_libs/<version>/include` is found from
`CUDA_HOME=<sdk>/cuda/<version>`. `OUT_DIR/report.txt` lists what was wrapped
and what was skipped.

## Extending

### Wrap a library by hand

Build a `cdylib` that depends on `tapir` (see `examples/posixio`):

```rust
use std::ffi::{c_char, c_int, c_void};

tapir::preload!(|b| {
    b.default_key("@bytes:log2; @count:log2");     // this library's bucketing
    b.from_env("bbv,summary");                     // TAPIR_PROFILERS, or this default
});

tapir::interpose! {
    lib "stdio";
    fn fopen(path: *const c_char => cstr "path", mode: *const c_char => cstr "mode") -> *mut c_void;
    fn fwrite(ptr: *const c_void => skip, size: usize => val "bytes", n: usize => val "count",
              f: *mut c_void => skip) -> usize;
    lib "stdlib";
    fn qsort(base: *mut c_void => skip, n: usize => val "count", size: usize => val "bytes",
             compar: *const c_void => sym);
}
```

Types must be ABI-identical to the C declaration; any C pointer can be
written `*const c_void`. Several tapir libraries can be preloaded together:
each forwards the C ABI to the next.

### Generate wrappers from headers

`tapir-gen` reads C headers in a `build.rs` (see `crates/tapir-rocm/build.rs`,
`examples/fakeblas/build.rs`) or from the command line:

```rust
let mut types = tapir_gen::TypeMap::c_basic();     // C's built-in types
types.handle("mylib_handle_t").enumeration("mylib_mode_t").scalar("mylib_int", "i32");
let mut g = tapir_gen::Generator::new(types);
g.rules.parse_text(tapir_vocab::LINALG.args)?;     // a shared vocabulary, optional
g.rules.add("mylib_open.name", "cstr@path")?;      // your own rules win
g.exclude("*_create*");
g.header("/path/mylib.h", "mylib", &["MYLIB_EXPORT"])?;   // [] = every prototype
// Export macro not the first word (`ret MYLIB_API name(...)`), types from the header itself:
// g.types.scan(&text); g.ignore("MYLIB_CALL"); g.source_marked(&text, "mylib", &["MYLIB_API"]);
std::fs::write(out_dir.join("wrappers.rs"), g.render())?;
```

```sh
tapir-gen --lib mylib --export MYLIB_EXPORT --type mylib_int=i32 \
          --rules crates/tapir-vocab/src/linalg.args --arg 'name=cstr@path' --report mylib.h > wrappers.rs
```

Without rules, by-value scalars are captured with `val` (C `char` with
`chr`), and pointers and handles are skipped. Parameter rules are
`selector action`. The selector is a parameter name or `function.name`
(globs), or a C kind (`:int`, `:float`, `:char`, `:enum`, `:handle`, `:ptr`).
The action is `[mode][@tag]`. Name rules beat kind rules; otherwise later
rules win. `g.fortran(true)` reads tagged scalar pointers with `deref`.
Functions with an unknown by-value type, or a mode that does not fit, are
skipped and listed by `g.report()`. Nothing is guessed.

### Add a vocabulary

A vocabulary is the knowledge one API family shares: a rules file for
`tapir-gen` and default key rules for the runtime. `crates/tapir-vocab` holds
`linalg` (BLAS/LAPACK-style: `@dim`, `@count`, `@stride`, `@flag`,
`@scalar`). Add `src/<name>.args` and a `Vocab` constant next to it, then use
it from a wrapper's `build.rs` (`g.rules.parse_text(V.args)`) and setup
(`b.default_key(V.key)`).

### Write a profiler

```rust
use tapir::{Ctx, Event, Format, Opts, Profiler, Sink, Table};

/// Total bytes written, from any argument tagged "bytes" times "count".
struct Volume { name: String, total: u64 }

impl Profiler for Volume {
    fn name(&self) -> &str { &self.name }
    fn needs_time(&self) -> bool { false }                 // skip clock reads
    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        if let Event::Call(c) = ev {
            let get = |t| c.tagged(t).filter_map(|v| v.as_i64()).next().unwrap_or(1) as u64;
            self.total += get("bytes") * get("count");
        }
    }
    fn finish(&mut self, out: &mut Sink) {
        let mut t = Table::new(&["bytes"]);
        t.push(vec![self.total.into()]);
        let _ = out.table(&self.name, Format::Csv, &t);
    }
}

tapir::preload!(|b| {
    b.factory("volume", |o: &Opts| Ok(Volume { name: o.name.clone(), total: 0 }));
    b.from_env("volume");
});
```

Optional hooks: `interval()` (online/periodic emission), `snapshot()` (text)
and `vector()` (numbers) for online consumers, and `wants_enter()` for a
callback before each call. `examples/fakeblas/src/lib.rs` has a complete
custom profiler with options. `tapir::from_fn("name", |ev| ...)` covers
one-off cases. `KeyPolicy` and `BlockTable` are public, so a new profiler gets
bucketing for free. `tapir::call(&SPEC, &args, || ...)` records calls from
ordinary Rust code without interposition.

### Feed tapir from other tools

Any code can push events into the active profilers through the C ABI
(`include/tapir.h`). It is resolved at runtime with `dlsym`, so no link-time
dependency is needed:

```c
#include "tapir.h"
const char *names[] = {"n:size", "dest:rank", "mode"};
int64_t vals[] = {n, dest, mode};
tapir_try_record("mylib", "transfer", 3, names, vals, t0, t1); // a "call" seen by every profiler
tapir_try_marker("timestep", 1);
tapir_try_interval(region);                       // close interval `region` in every profiler
n = tapir_try_vector("bbv", 1, ids, weights, cap); // or take bbv's vector and write it yourself
```

`tapir_record` covers anything with a name, integer arguments and a time
span: another tool's callbacks, asynchronous operations (GPU kernels from a
profiling API, I/O completions), hand-instrumented regions. Profilers can
tell such calls apart by `lib`.

## Layout

```
crates/tapir/        core: capture modes, interpose!, runtime, KeyPolicy/BlockTable, profilers, sinks, C ABI
crates/tapir-gen/    C header -> interpose! generator (library + CLI)
crates/tapir-vocab/  shared vocabularies (linalg)
crates/tapir-rocm/   libtapir_rocm.so: ROCm math libraries, generated from $ROCM_PATH headers, and the HIP runtime
crates/tapir-cuda/   libtapir_cuda.so: CUDA math libraries, generated from $CUDA_HOME headers, and the CUDA runtime;
                     test/run.sh checks it without a GPU or a CUDA toolkit
include/tapir.h      C ABI + runtime-resolved helpers
examples/posixio/    hand-written wrappers for stdio and qsort (strings, callbacks); run.sh checks them
examples/fakeblas/   GPU-free BLAS-like test library: generated wrappers + a custom profiler
integrations/nuggets Nuggets analysis hook with GPU vectors, and an end-to-end test
python/tapir_read.py readers for blocks and .bb files; offline re-bucketing by tag
bin/tapir-run        launcher
```

## Limits

- Only calls that cross a shared-library boundary can be interposed. Static
  libraries and calls inside one library are not visible. Calls between
  wrapped libraries are visible and marked `depth > 0`.
- `interpose!` cannot wrap variadic functions (`printf`, `open`) or pass
  structs by value; `tapir-gen` reports and skips both.
- `tapir_record` carries integer arguments only.
- The header parser is simple (no preprocessor). It is enough for headers
  that mark exports with a macro; for others, list prototypes or wrap by hand.
- `tapir-rocm` assumes LP64 `rocblas_int` (`i32`), and times asynchronous
  calls at launch unless `TAPIR_GPU_SYNC=device`. The first rocBLAS call also
  includes library initialisation (~0.2 s on an MI210).
- `tapir-cuda` sees the calls a program makes into `libcudart.so` and the
  math libraries' shared objects. A program linked against the static
  libraries (`-lcublas_static`, `-cudart static`) makes none; link the shared
  ones. Launches inside cuBLAS/cuSOLVER/cuSPARSE are not visible (they carry
  their own runtime), which matches `tapir-rocm`, where they are nested and
  not recorded. The generic cuSPARSE and rocSPARSE APIs pass sizes inside
  descriptors, so their blocks are keyed by name and options only. Functions
  that take a complex number by value (12 in CUDA 12.9, all in cuSOLVER's
  sparse eigenvalue API) are not wrapped.
- Tested against real rocBLAS/rocSOLVER (ROCm 6.4.2, MI210 on a Frontier
  login node) and glibc stdio; not yet with a full Nuggets-instrumented app on
  a compute node. `tapir-cuda` is generated and type-checked against the CUDA
  12.9 headers (cuBLAS 12.9.1, cuSOLVER 11.7.5, cuSPARSE 12.5.10) and tested
  against a fake CUDA library on x86-64 and aarch64 (`crates/tapir-cuda/test/run.sh`);
  not yet run on a GPU.
