# Driving tapir from the application

So far tapir has run on its own: the program was not changed, and output
appeared at exit. Many uses need the application's help:

- **Phase analysis** needs intervals that line up with the application's
  own structure (time steps, regions, iterations), not arbitrary call
  counts.
- **Online consumers**, such as a simulation-point tool, want the current
  vector *during* the run, to store next to their own data.
- **Regions of interest**: record only the solve, not the setup.
- **Work tapir cannot see**, such as a phase of pure computation or a
  callback from another tool, should still appear in the profile.

All of this goes through tapir's C ABI, declared in `include/tapir.h`.

## The `tapir_try_*` helpers

`tapir.h` declares `static inline` helpers that look up tapir's functions
at run time with `dlsym`. If no tapir library is loaded, they do nothing
and return `-1` or `0`. Nothing changes at link time, so **the same binary
runs with and without tapir**. On glibc older than 2.34, add `-ldl`.

| Helper | Does |
|---|---|
| `tapir_try_active()` | 1 if a tapir runtime is loaded |
| `tapir_try_enable(on)` | pause (0) or resume recording |
| `tapir_try_interval(index)` | close the current interval in every profiler (`-1`: the next index) |
| `tapir_try_snapshot(name, take, buf, cap)` | profiler `name`'s current interval as text |
| `tapir_try_vector(name, take, ids, weights, cap)` | profiler `name`'s current interval as a sparse vector |
| `tapir_try_marker(name, begin)` | a named begin/end marker, delivered to every profiler |
| `tapir_try_record(lib, func, n, names, values, t0, t1)` | report a "call" tapir did not wrap |
| `tapir_try_now_ns()` | tapir's clock, for `tapir_try_record` timestamps |
| `tapir_try_flush()` | write end-of-run output now |

## Step 1: instrument the thumbnailer

Copy `app.c` to `app2.c` and add the calls:

```c
/* The thumbnailer again, now driving tapir online. */
#include <stdio.h>
#include <stdint.h>
#include "pix.h"
#include "tapir.h"

int invert(int v) { return 255 - v; }
static int clamp(int v) { return v > 255 ? 255 : v; }

int main(void) {
    const char *files[] = {"cat.png", "dog.png", "bird.png"};
    tapir_try_enable(1);                       /* start recording here (TAPIR_START=paused) */
    for (int phase = 0; phase < 2; phase++) {
        int size = phase == 0 ? 128 : 1000;
        tapir_try_marker("phase", 1);
        uint64_t t0 = tapir_try_now_ns();
        for (int i = 0; i < 3; i++) {
            pix_image img = pix_load(files[i]);
            pix_resize(img, size, size + i, PIX_BILINEAR);
            pix_blur(img, 1.5f + i);
            pix_convert(img, 'g');
            pix_map(img, i % 2 ? invert : clamp);
            pix_save(img, "out.png");
            pix_free(img);
        }
        /* Report work tapir cannot see: here, a whole phase as one "call". */
        const char *names[] = {"phase", "size:size"};
        int64_t values[] = {phase, size};
        tapir_try_record("app", "phase", 2, names, values, t0, tapir_try_now_ns());
        tapir_try_marker("phase", 0);

        char text[256];
        if (tapir_try_snapshot("pixels", 0, text, sizeof text) >= 0)
            printf("phase %d: %s", phase, text);
        uint64_t ids[64];
        double weights[64];
        int64_t n = tapir_try_vector("bbv", 0, ids, weights, 64);
        printf("phase %d: bbv has %lld blocks\n", phase, (long long)n);
        tapir_try_interval(-1);                /* close interval `phase` everywhere */
    }
    printf("done (tapir %s)\n", tapir_try_active() ? "active" : "absent");
    return 0;
}
```

Build it with tapir's `include/` directory on the include path:

```sh
gcc -O1 -rdynamic -o app2 app2.c -I../tapir/include -L. -lpix -Wl,-rpath,'$ORIGIN'
```

## Step 2: run without tapir

```sh
$ ./app2
phase 0: bbv has -1 blocks
phase 1: bbv has -1 blocks
done (tapir absent)
```

Every helper is a no-op. The snapshot returned `-1`, so nothing was
printed for it.

## Step 3: run with tapir

```sh
$ TAPIR_VERBOSE=1 TAPIR_START=paused \
  TAPIR_PROFILERS='bbv, pixels(min=100000), trace(fn=phase)' \
  LD_PRELOAD=../tapir-pix/target/release/libtapir_pix.so ./app2
[tapir] pid 171 profilers: [bbv, pixels, trace]
phase 0: 0 resizes, 0 pixels
phase 0: bbv has 11 blocks
phase 1: 3 resizes, 3003000 pixels
phase 1: bbv has 11 blocks
done (tapir active)
[tapir] wrote tapir-out/bbv.p171.bb
[tapir] wrote tapir-out/trace.p171.csv
[tapir] wrote tapir-out/bbv.blocks.p171.csv
[tapir] wrote tapir-out/pixels.p171.csv
```

Here is what each part did.

**`TAPIR_START=paused` and `tapir_try_enable(1)`.** Recording starts
paused, and the application turns it on where it chooses. While paused,
wrapped calls go straight to the real function at a cost of a few
nanoseconds. Call `tapir_try_enable(0)` to pause again.

**`tapir_try_interval(-1)`.** This closed interval 0, then interval 1, in
*every* profiler. Each one wrote its part:

```text
$ cat tapir-out/bbv.*.bb
T:1:1 :2:3 :3:3 :4:3 :5:2 :6:3 :7:3 :8:1 :9:1 :10:1 :11:1
T:1:1 :3:3 :4:3 :5:2 :6:3 :7:3 :8:1 :9:1 :10:1 :12:3 :13:1
$ cat tapir-out/pixels.*.csv
interval,resizes,megapixels
0,0,0
1,3,3.0029999999999997
```

Line *N* of the `.bb` file is interval *N* of the application. `bbv`
writes a line even for an empty interval, so the two stay aligned. The
phases differ only in the `pix_resize` block (2 vs. 12) and the `phase`
record (11 vs. 13).

**`tapir_try_record`.** The `phase` "call" is in the profile like any
wrapped call. A name written `"size:size"` gets the tag `size`, so the
library's `@size:log2` rule bucketed it:

```text
11,...,app,phase,"phase(phase=0,size=128..255)",phase=0 size@size=128..255,1,1
13,...,app,phase,"phase(phase=1,size=512..1023)",phase=1 size@size=512..1023,1,1
```

**`tapir_try_marker`.** Markers reach every profiler as `Event::Marker`.
`trace` records them:

```text
t_ns,duration_ns,thread,depth,interval,kind,lib,function,args
130443695915,0,0,0,0,marker,,phase,begin
130443697540,96333,0,0,0,call,app,phase,phase=0 size=128
130443797915,0,0,0,0,marker,,phase,end
130443927540,0,0,0,1,marker,,phase,begin
130443927957,5265059,0,0,1,call,app,phase,phase=1 size=1000
130449197807,0,0,0,1,marker,,phase,end
```

**`tapir_try_snapshot`** called the `snapshot` method you wrote for
`pixels`. With `take = 0` it only peeks.

**`tapir_try_vector`** returned `bbv`'s current interval as
`(ids[i], weights[i])` pairs. For `bbv`, the ids are the stable 64-bit
block hashes from `bbv.blocks.*.csv`.

## Taking data instead of peeking

With `take = 1`, `tapir_try_vector` and `tapir_try_snapshot` close the
profiler's interval **without** tapir writing it. The caller now owns that
data. This is how an online tool stores the GPU vector next to its own CPU
data for the same region:

```c
uint64_t ids[4096];
double weights[4096];
int64_t n = tapir_try_vector("bbv", 1, ids, weights, 4096);
if (n > 4096) {
    /* Too many blocks: nothing was taken. Grow the buffers to n and retry. */
}
for (int64_t i = 0; i < n && i < 4096; i++)
    fprintf(out, "%llu:%g ", (unsigned long long)ids[i], weights[i]);
```

`take` only closes the named profiler's interval. `tapir_try_interval`
closes them all. `integrations/nuggets/analysis.c` is a complete real
example.

## Letting tapir close intervals itself

When you cannot change the application, `bbv` can close its own intervals:

```sh
TAPIR_PROFILERS='bbv(every=calls:1000)'   # every 1000 recorded calls
TAPIR_PROFILERS='bbv(every=time:0.5)'     # every 0.5 s of wall time (checked at each call)
```
