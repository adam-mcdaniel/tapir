# Limitations and troubleshooting

## Limitations

- **Only calls across a shared-library boundary can be interposed.** Calls
  into static libraries, and calls between functions inside one library,
  are invisible. Calls between two wrapped libraries are visible and
  marked `depth > 0`.
- **Linux with glibc only.** tapir relies on `LD_PRELOAD`, ELF
  `.init_array` constructors and glibc's `RTLD_NEXT`.
- **No variadic functions** (`printf`, `open`) and **no structs by value**
  in `interpose!`. `tapir-gen` reports and skips both. You can write such
  wrappers by hand around `tapir::call`; see
  [the GPU case study](../guides/gpu-libraries.md#cudartrs-hand-written-wrappers-next-to-generated-ones).
- **`tapir_record` carries integer arguments only.**
- **The header parser has no preprocessor.** It is enough for headers that
  mark exports with a macro. For others, list the prototypes in a file or
  wrap them by hand.
- **Asynchronous APIs are timed at launch** unless the wrapper waits in an
  `after_call` hook (`TAPIR_GPU_SYNC=device` for the GPU libraries).

## Troubleshooting

### No `tapir-out/` directory appears

- Run with `TAPIR_VERBOSE=1`. If no `[tapir] pid ... profilers:` line
  appears, the library was not loaded: check the `LD_PRELOAD` path. A
  relative path is resolved against each process's working directory, so
  use an absolute one (`tapir-run -l` does this for you).
- If the profilers line appears but nothing is written, no wrapped call
  was recorded:
  - Is the program linked against the *shared* library? Check with
    `ldd ./app`. Static linking (`-lfoo_static`, `-cudart static`) makes
    interposition impossible.
  - Does the program call the function you wrapped, or a variant
    (`cublasDgemm_v2` rather than `cublasDgemm`, a `_ptsz` or `64`
    suffix)? `nm -D --undefined-only ./app` lists what it imports.
  - Is recording paused (`TAPIR_START=paused` with no `tapir_enable(1)`)?
  - Is a `fn=` or `lib=` filter excluding everything?

### ``[tapir] fatal: no definition of `foo` after the interposer``

The application called `foo`, and the wrapper library declares it, but no
library loaded *after* the wrapper defines it. Usually the real library
is not loaded at all, or the function only exists in a newer version.
Remove the declaration, or make sure the real library is loaded.

### `[tapir] error: unknown profiler ...` / `unknown bucket ...`

A typo in `TAPIR_PROFILERS` or `TAPIR_KEY`. The error lists the known
profiler names. Errors never stop the application; the profiler with the
error is left out. When several tapir libraries are loaded, a custom
profiler registered by only one of them produces this error from the
others, which is harmless.

### The program crashes only under tapir

- **Wrong types in `interpose!`.** The wrapper forwards the arguments it
  declares. A mismatch (an `i32` declared for a `size_t`, a missing
  parameter) breaks the real call. Compare against the header, or generate
  the declarations with `tapir-gen`.
- **`deref` or `cstr` on memory that isn't readable host memory**, such as a
  device pointer or an uninitialized output pointer. Use `skip`.

### Callbacks show as `app+0x1234` instead of a name

The function has no dynamic symbol. Link the executable with `-rdynamic`,
or accept the offset: it is stable across runs of the same binary. `static`
functions never have names.

### Too many blocks / every call is its own block

The key is too fine. Look at the `args` column of `bbv.blocks.*.csv` to
see which argument varies, then bucket or drop it (`TAPIR_KEY='@size:log2'`,
`'myfn.seed:drop'`). Untagged arguments are exact by default, and `@:drop`
drops them all.

### Output files have `.1` suffixes

A file with that name already existed. tapir never overwrites output.
This happens on repeated runs with a fixed `TAPIR_OUT`, or when several
tapir libraries in one process write identically named streams.

### Durations of GPU or other asynchronous calls are tiny

They measure the launch. Use `TAPIR_GPU_SYNC=device` (GPU libraries), or
`b.after_call(...)` in your own wrapper, if you need execution time.
`bbv` with `weight=calls` does not need timing at all.
