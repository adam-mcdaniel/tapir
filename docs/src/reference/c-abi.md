# The C ABI

Every tapir library exports these functions. They are declared in
`include/tapir.h`. When several tapir libraries are loaded, each function
handles its own runtime and forwards to the next library.

Call them through the `tapir_try_*` helpers in the same header. Those look
the symbols up with `dlsym(RTLD_DEFAULT, ...)` on each call until found,
and do nothing (returning `-1` or `0`) when no tapir library is loaded.
They need nothing at link time (`-ldl` on glibc older than 2.34). To link
against a tapir library directly instead, define `TAPIR_DIRECT` before
including the header to get plain prototypes.

| Function | Returns | Description |
|---|---|---|
| `int tapir_active(void)` | 1 or 0 | 1 if a tapir runtime is initialized in this process |
| `void tapir_enable(int on)` | | pause (0) or resume (non-zero) recording |
| `int64_t tapir_interval(int64_t index)` | index closed, or -1 | close the current interval in every profiler; `index < 0` means the next one |
| `int64_t tapir_snapshot(const char *name, int take, char *buf, size_t cap)` | text length, or -1 | copy profiler `name`'s current interval as text into `buf` (always NUL-terminated if `cap > 0`); call with `cap = 0` to get the size; `take != 0` also closes that interval without tapir writing it |
| `int64_t tapir_vector(const char *name, int take, uint64_t *ids, double *weights, size_t cap)` | pair count, or -1 | profiler `name`'s current interval as up to `cap` `(ids[i], weights[i])` pairs; with `take`, if there are more than `cap` pairs nothing is taken and the needed count is returned |
| `void tapir_marker(const char *name, int begin)` | | deliver a begin (non-zero) or end marker to every profiler |
| `void tapir_record(const char *lib, const char *func, int nargs, const char *const *arg_names, const int64_t *arg_values, uint64_t t_start, uint64_t t_end)` | | report a call tapir did not wrap; names may be `"name:tag"`; times from `tapir_now_ns()`, or `0, 0` for now |
| `void tapir_flush(void)` | | write end-of-run output now (it is otherwise written at exit; idempotent) |
| `uint64_t tapir_now_ns(void)` | ns | tapir's clock: `CLOCK_MONOTONIC` in nanoseconds |

`-1` from `tapir_snapshot` or `tapir_vector` means no loaded tapir library
has a profiler with that name, or that profiler has no text or vector form.
Of the built-ins, `bbv` has both. `summary` and `trace` have neither.

For `bbv`, `tapir_vector` ids are the stable 64-bit block hashes (the
`hash` column of `bbv.blocks.*.csv`), the same across ranks and runs.

## Example: sample at region boundaries

```c
#include "tapir.h"

static uint64_t *ids;
static double *weights;
static size_t cap = 1024;

void on_region_end(int region, FILE *out) {
    if (!ids) { ids = malloc(cap * sizeof *ids); weights = malloc(cap * sizeof *weights); }
    int64_t n = tapir_try_vector("bbv", 1, ids, weights, cap);
    while (n > (int64_t)cap) {                        /* too small: nothing was taken */
        cap = (size_t)n * 2;
        ids = realloc(ids, cap * sizeof *ids);
        weights = realloc(weights, cap * sizeof *weights);
        n = tapir_try_vector("bbv", 1, ids, weights, cap);
    }
    if (n < 0) return;                                /* tapir not loaded */
    fprintf(out, "gpu_bb_id,%d", region);
    for (int64_t i = 0; i < n; i++) fprintf(out, ",%llu", (unsigned long long)ids[i]);
    fprintf(out, "\ngpu_bbv,%d", region);
    for (int64_t i = 0; i < n; i++) fprintf(out, ",%g", weights[i]);
    fputc('\n', out);
}
```

The rows this writes are what `tapir_read.read_vector_rows(path, "gpu_")`
reads.
