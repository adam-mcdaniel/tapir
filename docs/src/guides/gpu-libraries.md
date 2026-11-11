# Case study: the GPU libraries

`tapir-rocm` and `tapir-cuda` are production wrapper libraries built with
the same techniques as the tutorial. They are worth reading before you wrap
a large API. This chapter walks through `tapir-cuda`. `tapir-rocm` is
structured the same way, with `hip.rs` in place of `cudart.rs`.

## Using them

```sh
cargo build --release -p tapir-cuda          # needs CUDA headers; CUDA_HOME
bin/tapir-run -p bbv,summary -- ./app        # finds target/release/libtapir_cuda.so
```

| | `tapir-rocm` (AMD) | `tapir-cuda` (NVIDIA) |
|---|---|---|
| Math libraries, generated from the installed headers | rocBLAS, rocSOLVER, hipBLAS, hipSOLVER, rocSPARSE | cuBLAS, cuSOLVER (Dn, Sp, Rf), cuSPARSE |
| Runtime: allocation, free, copies, memsets | `hipMalloc`, `hipMemcpy`, ... | `cudaMalloc`, `cudaMemcpy`, ... |
| Kernel launches, as kernel + blocks + threads | `hipLaunchKernel`, ... | `cudaLaunchKernel`, `cudaLaunchKernelExC`, ... |
| Toolkit root at build time | `ROCM_PATH` | `CUDA_HOME` (or `CUDA_PATH`); `TAPIR_CUDA_INCLUDE_DIRS=a:b` |
| Libraries to wrap | `TAPIR_ROCM_LIBS=rocblas,rocsolver,...` | `TAPIR_CUDA_LIBS=cublas,cusolver,cusparse` (`none`: runtime only) |
| Extra name globs, `;`-separated | `TAPIR_ROCM_INCLUDE` / `TAPIR_ROCM_EXCLUDE` | `TAPIR_CUDA_INCLUDE` / `TAPIR_CUDA_EXCLUDE` |
| Wait for the GPU after each call | `TAPIR_GPU_SYNC=device` | `TAPIR_GPU_SYNC=device` |

`OUT_DIR/report.txt`, under `target/release/build/tapir-cuda-*/out/`,
lists what was wrapped and what was skipped.

## Anatomy of `tapir-cuda`

```text
crates/tapir-cuda/
├── build.rs          finds the headers, generates wrappers.rs + report.txt
├── src/lib.rs        preload! setup, includes wrappers.rs
├── src/cudart.rs     hand-written CUDA runtime wrappers
└── test/             a fake CUDA library + app, to test without a GPU
```

### `build.rs`: generating from the installed headers

The wrappers are generated at build time from whatever CUDA version is
installed, so they always match it. These are the steps, each something
you saw in the tutorial:

1. **Find the headers.** Look in `TAPIR_CUDA_INCLUDE_DIRS`, then
   `$CUDA_HOME/include`, then the NVIDIA HPC SDK layout. Print
   `cargo:rerun-if-env-changed` for every variable read and
   `cargo:rerun-if-changed` for every header.
2. **Rules.** Load the shared `linalg` vocabulary, then CUDA-specific
   additions such as `batchsize @count`, `nnz* @dim` and `reorder @flag`.
3. **Exclude noise.** Setup and query APIs are not work and only clutter a
   profile: `*Create*`, `*Destroy*`, `*Set*`, `*Get*`, `*bufferSize*`, and
   so on. Users can add more globs through environment variables.
4. **Types from the headers.** Rather than listing hundreds of handles and
   enums by hand, call `g.types.scan(&text)` on every header (including
   type-only headers such as `library_types.h`) *before* parsing any
   prototypes, because the libraries use each other's types.
5. **Prototypes.** CUDA headers write `cublasStatus_t CUBLASWINAPI
   cublasDgemm_v2(...)` with the export macro in the middle, so use
   `g.source_marked(text, lib, &["CUBLASAPI"])`, with
   `g.ignore("CUBLASWINAPI")` to remove the calling-convention macro.
6. **Fail loudly.** If nothing was found, `panic!` with the directories
   searched and how to fix it.
7. **Write** `wrappers.rs` and `report.txt` to `OUT_DIR`.

`tapir-rocm` lists its handles and enums explicitly instead (`rocm_types()`),
because its headers are regular enough for that, and it reads rocSPARSE's
one-header-per-routine layout by walking a directory.

### `lib.rs`: setup with a post-call hook

```rust
static CUDA_SYNC: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn cuda_device_sync() {
    let mut p = CUDA_SYNC.load(Ordering::Relaxed);
    if p.is_null() {
        p = tapir::interpose::lookup_global(c"cudaDeviceSynchronize");
        if p.is_null() {
            return;
        }
        CUDA_SYNC.store(p, Ordering::Relaxed);
    }
    let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
    unsafe { f() };
}

tapir::preload!(|b| {
    b.default_key(tapir_vocab::LINALG.key);
    b.from_env("bbv,summary");
    if std::env::var("TAPIR_GPU_SYNC").is_ok_and(|v| v == "device" || v == "1") {
        b.after_call(cuda_device_sync);
    }
});

include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));
mod cudart;
```

GPU calls are asynchronous: a timer around `cublasDgemm` measures the
launch, not the work. `b.after_call(hook)` runs `hook()` after every
recorded call, *inside* the timed region, so with `TAPIR_GPU_SYNC=device`
the durations include execution. The hook is a plain `fn()`, and it looks
up `cudaDeviceSynchronize` at run time with `lookup_global`, so the
wrapper library does not link against CUDA. Calls the hook makes are not
recorded.

This pattern works for any library with deferred work: flush, wait or
synchronize in `after_call`, and make it optional, because it changes the
application's behaviour.

### `cudart.rs`: hand-written wrappers next to generated ones

The runtime functions are few and stable, so they are written by hand with
`interpose!`. Byte counts are tagged `dim` and copy directions `flag`, to
fit the `linalg` vocabulary:

```rust
tapir::interpose! {
    lib "cuda";
    fn cudaMalloc(ptr: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn cudaMemcpy(dst: *mut c_void => skip, src: *const c_void => skip,
                  size: usize => val "dim", kind: i32 => val "flag") -> i32;
    // ...
}
```

Two techniques here are useful elsewhere:

**Wrapping symbol variants with a macro.** Programs built with
per-thread default streams call `cudaMemcpyAsync_ptsz` instead of
`cudaMemcpyAsync`. A small `macro_rules!` expands each declaration into
both names:

```rust
macro_rules! with_ptsz {
    ( $( fn $name:ident / $ptsz:ident ( $($args:tt)* ) -> i32; )* ) => {
        tapir::interpose! {
            lib "cuda";
            $( fn $name( $($args)* ) -> i32; fn $ptsz( $($args)* ) -> i32; )*
        }
    };
}
```

**Capturing what modes cannot express.** A kernel launch passes `dim3`
structs *by value*, and the interesting numbers are products of their
fields. `interpose!` cannot do that, so the launch wrappers are written out
by hand on top of the same building blocks the macro uses:

```rust
const LAUNCH_ARGS: &[ArgSpec] = &[ArgSpec::new("kernel", "flag"),
                                  ArgSpec::new("blocks", "dim"),
                                  ArgSpec::new("threads", "dim")];

#[no_mangle]
pub unsafe extern "C" fn cudaLaunchKernel(f: *const c_void, grid: Dim3, block: Dim3,
                                          args: *mut *mut c_void, shared: usize,
                                          stream: *mut c_void) -> i32 {
    static SPEC: FnSpec = FnSpec::new("cuda", "cudaLaunchKernel", LAUNCH_ARGS);
    static REAL: tapir::interpose::RealFn = tapir::interpose::RealFn::new(c"cudaLaunchKernel");
    type Real = unsafe extern "C" fn(*const c_void, Dim3, Dim3, *mut *mut c_void, usize, *mut c_void) -> i32;
    let real: Real = std::mem::transmute::<*mut c_void, Real>(REAL.get());
    let values = [Value::Sym(f as usize), Value::Int(grid.total()), Value::Int(block.total())];
    tapir::call(&SPEC, &values, move || real(f, grid, block, args, shared, stream))
}
```

`Dim3` is a `#[repr(C)]` struct matching CUDA's `dim3`. `RealFn` resolves
the next definition once and caches it. The kernel is captured as `Sym`,
so it is reported by its host-stub symbol name.

### `test/`: testing without the real thing

`test/run.sh` builds `tapir-cuda` against two cuBLAS prototypes in
`test/include/` (`TAPIR_CUDA_INCLUDE_DIRS`, `TAPIR_CUDA_LIBS=cublas`) and
a fake `libfakecuda.so` written in C with the same symbols. Every fake
function prints the arguments it received, so the test can check that
arguments pass through the wrappers intact, including `dim3` by value. An
application calls the fake library under tapir, and `check.py` checks both
the printed arguments and the profile. All of this runs on any Linux
machine, including CI and login nodes without GPUs. `examples/fakeblas`
does the same for the generated path. Do the same for your own wrappers.

## Limits specific to GPUs

- A program linked against static libraries (`-lcublas_static`,
  `-cudart static`) makes no calls that can be interposed. Link the shared
  ones.
- Launches *inside* cuBLAS, cuSOLVER and cuSPARSE are not visible: they
  carry their own copy of the runtime.
- The generic cuSPARSE and rocSPARSE APIs pass sizes inside descriptors,
  so their blocks are keyed by name and options only.
- Functions that take a complex number by value are skipped.
- Block names and hashes are each vendor's own (`rocsolver_zgetrf` vs.
  `cusolverDnZgetrf`). Profiles are not matched across vendors.
