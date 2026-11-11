# Events from other sources

Interposition is only one way to feed tapir. Profilers see three kinds of
event, and you can produce all of them yourself.

## From C: `tapir_record`

`tapir_record` reports anything with a name, integer arguments and a time
span. Profilers see it as an ordinary call of `func` in library `lib`:

```c
#include "tapir.h"

uint64_t t0 = tapir_try_now_ns();
do_transfer(buf, n, dest);
uint64_t t1 = tapir_try_now_ns();

const char *names[] = {"n:bytes", "dest:rank", "mode"};   /* "name:tag" or "name" */
int64_t values[]    = {n, dest, mode};
tapir_try_record("mpi", "transfer", 3, names, values, t0, t1);
```

- `name:tag` tags an argument. Bare names are untagged.
- Timestamps must come from `tapir_now_ns()` (`CLOCK_MONOTONIC`
  nanoseconds, the clock most tracing tools use). Pass `0, 0` to stamp
  "now" with zero duration.
- Arguments are integers only. Recorded calls have `depth` 0.
- Nothing is recorded while recording is paused.

Typical uses:

- **Asynchronous work** observed elsewhere: GPU kernels from a vendor
  profiling API, I/O completions, with their real start and end times.
- **Another tool's callbacks**: forward them so one profile covers both.
- **Hand-instrumented regions** in code that is not in a shared library.

Profilers can tell these calls apart by `lib`, so pick a library tag that
differs from your wrappers' tags (`summary(lib=mpi)`, `bbv(lib=pix)`).

## From C: markers

```c
tapir_try_marker("solve", 1);   /* begin */
solve();
tapir_try_marker("solve", 0);   /* end */
```

Markers reach every profiler as `Event::Marker { name, begin, t }`.
`trace` writes them as rows. `bbv` and `summary` ignore them. Your own
profiler can use them, for example to attribute calls to regions.

## Instrumenting Rust code with `tapir::call`

`tapir::call` is the function every generated wrapper calls. It works on
any Rust code. Describe the "function" once in a `static FnSpec`, then wrap
the work in a closure:

```rust
use tapir::{ArgSpec, FnSpec, Value};

static RENDER: FnSpec = FnSpec::new("game", "render", &[
    ArgSpec::new("width", "size"),     // (parameter name, tag)
    ArgSpec::new("height", "size"),
    ArgSpec::new("quality", ""),       // untagged
]);

fn render(width: u32, height: u32, quality: u8) -> u64 {
    let args = [Value::UInt(width as u64), Value::UInt(height as u64), Value::Char(quality)];
    tapir::call(&RENDER, &args, || {
        // the real work; its return value is passed through
        expensive_render(width, height, quality)
    })
}
```

`args` must have one `Value` per `ArgSpec`, in the same order. Nothing
is recorded until the runtime is initialized. In a `cdylib` loaded with
`LD_PRELOAD`, `preload!` does that. In an ordinary program, initialize it
yourself at the start of `main`:

```rust
fn main() {
    tapir::init(|b| {
        b.default_key("@size:log2");
        b.from_env("summary");
    });
    // ... calls to render() are now recorded ...
    tapir::runtime().unwrap().finish();   // optional: tapir also writes at exit
}
```

`tapir::init` runs once; later calls are ignored and return `false`. The
runtime is process-global.

This is also how to capture what the capture modes cannot express. Write
the `#[no_mangle] extern "C"` wrapper yourself, look up the real function
with `tapir::interpose::lookup_next(c"name")`, build the `Value`s however
you like (a field from a struct, a computed byte count), and call
`tapir::call`.

## From Rust: the runtime handle

`tapir::runtime()` returns the `Runtime` once it is initialized. It has the
same operations as the C ABI:

| Method | C equivalent |
|---|---|
| `rt.set_enabled(on)` / `rt.is_enabled()` | `tapir_enable` |
| `rt.close_interval(None)` | `tapir_interval(-1)` |
| `rt.snapshot(name, take)` | `tapir_snapshot` |
| `rt.vector(name, take, cap)` | `tapir_vector` |
| `rt.dispatch(&Event::Marker { .. }, false)` | `tapir_marker` |
| `rt.with_profiler(name, \|p\| ...)` | (direct access to one profiler) |
| `rt.finish()` | `tapir_flush` |
