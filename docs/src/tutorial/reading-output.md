# Reading the output

This chapter describes every file the built-in profilers write and shows
how to load them in Python.

## Where files go

The path comes from a template, `TAPIR_OUT`, which defaults to
`tapir-out/{stream}.{tag}.{ext}`:

| Placeholder | Becomes |
|---|---|
| `{stream}` | the profiler's stream name: `bbv`, `bbv.blocks`, `summary`, `pixels`, ... |
| `{tag}` | `r<rank>` under MPI, otherwise `p<pid>` |
| `{rank}` | the MPI rank, or `x` |
| `{pid}` | the process id |
| `{exe}` | the executable's file name |
| `{ext}` | `csv`, `tsv`, `jsonl`, or `bb` |

Directories are created as needed. **Existing files are never
overwritten.** If the path exists, `.1`, `.2`, ... is added before the
extension:

```sh
$ TAPIR_OUT='prof/{exe}-{stream}.{ext}' TAPIR_FORMAT=jsonl TAPIR_PROFILERS=summary LD_PRELOAD=$LIB ./app
$ TAPIR_OUT='prof/{exe}-{stream}.{ext}' TAPIR_FORMAT=jsonl TAPIR_PROFILERS=summary LD_PRELOAD=$LIB ./app
$ ls prof
app-summary.1.jsonl  app-summary.jsonl
```

A process that never records an event writes nothing. That covers shells
and helper programs that inherit `LD_PRELOAD` from the command you
profiled.

## Formats

Table output can be `csv` (the default), `tsv` or `jsonl` (one JSON object
per row). Set it with `TAPIR_FORMAT` (for `summary`, `trace` and custom
profilers that use `Opts::format`), or per profiler with `format=`. `bbv`
writes `.bb` unless given its own `format=`. CSV fields that contain `,`, `"` or a newline are quoted.

```json
{"lib":"pix","function":"pix_resize","signature":"pix_resize(width=1000,height=1000,filter=1)","calls":1,"nested_calls":0,"total_s":0.000904878,"mean_s":0.000904878,"min_s":0.000904878,"max_s":0.000904878}
```

## The files

### `summary.<tag>.csv`

| Column | Meaning |
|---|---|
| `lib`, `function` | library tag and function |
| `signature` | the call's label under the summary's key (exact by default) |
| `calls` | number of calls |
| `nested_calls` | how many of them were made from inside another wrapped call |
| `total_s`, `mean_s`, `min_s`, `max_s` | durations in seconds |

Rows are sorted by `total_s`, largest first. With `every=N`, `summary`
also writes `summary.partial<index>` every *N* intervals.

### `trace.<tag>.csv`

| Column | Meaning |
|---|---|
| `t_ns` | start time, `CLOCK_MONOTONIC` nanoseconds |
| `duration_ns` | duration |
| `thread` | per-process thread index (0 = first thread seen) |
| `depth` | 0 for application calls, >0 for nested calls |
| `interval` | interval open at the time |
| `kind` | `call` or `marker` |
| `lib`, `function` | library tag and function (marker name for markers) |
| `args` | `name=value` for each captured argument (`begin`/`end` for markers) |

Rows are written in batches of 4096 (`batch=`) and at every interval.

### `bbv.<tag>.bb`

SimPoint's frequency-vector format, one line per interval, with
`:block:weight` pairs using the dense 1-based block ids:

```text
T:1:1 :2:3 :3:3 :4:3 :5:2 :6:3 :7:3 :8:1 :9:1 :10:1 :11:1
```

With `weight=time`, weights are nanoseconds instead of call counts. With
`format=csv|tsv|jsonl`, `bbv` writes a long table instead
(`interval,block,hash,weight`).

### `bbv.blocks.<tag>.csv`

| Column | Meaning |
|---|---|
| `block` | dense id, as in the `.bb` file |
| `hash` | stable 64-bit id (FNV-1a of `label`), the same across runs and ranks |
| `lib`, `function` | library tag and function |
| `label` | `function(arg=bucket,...)` |
| `args` | space-separated `name@tag=bucket` (or `name=bucket` if untagged) |
| `calls`, `weight` | totals over the whole run |

Bucketed values render as inclusive ranges (`512..1023`), approximations
(`~1024`), or exact values. Strings are percent-escaped so labels never
contain spaces, commas, parentheses or `=`.

## Reading it in Python

`python/tapir_read.py` uses only the standard library:

```python
import sys
sys.path.insert(0, "tapir/python")
import tapir_read as tr

blocks = tr.read_blocks("tapir-out/bbv.blocks.p171.csv")   # {hash: Block}
vectors = tr.read_bb("tapir-out/bbv.p171.bb")              # [{block: weight}, ...] per interval

b = next(iter(blocks.values()))
print(b)
# Block(block=1, hash=660367942422571909, lib='pix', function='pix_load',
#       label='pix_load(path=cat.png)', args={'path': ('path', 'cat.png')}, calls=2, weight=2)
```

`Block.args` maps each parameter name to `(tag, value)`, with strings
unescaped.

### Re-bucketing offline

Choosing a bucket size is a trade-off you may not want to make before the
run. Record with exact keys, then try different groupings afterwards
without running again:

```sh
tapir-run -l $LIB -p 'bbv(key=exact)' -- ./app
```

```python
from collections import Counter

blocks = tr.read_blocks("tapir-out/bbv.blocks.p156.csv")
# Two significant bits for sizes; ignore radius and path entirely.
key = tr.rebucket(blocks, size=lambda v: tr.relbucket(v, 2), radius=None, path=None)
print(Counter(key.values()).most_common(4))
# [(('pix_load', ()), 3),
#  (('pix_resize', (('width', 128), ('height', 128), ('filter', 1))), 3),
#  (('pix_blur', ()), 3),
#  (('pix_resize', (('width', 1024), ('height', 1024), ('filter', 1))), 3)]
```

`rebucket` takes one keyword per tag: a function applied to the integer
value, or `None` to drop that tag. Blocks that map to equal tuples belong
to the same coarser block. `relbucket(v, bits)` is the same rounding as the
`sig/<bits>` key bucket.

`tr.read_vector_rows(path, prefix)` reads vectors that an online driver
wrote itself (`<prefix>bb_id,<region>,ids...` and
`<prefix>bbv,<region>,weights...` row pairs, as the Nuggets hook writes
them).

That completes the tutorial. You have wrapped a library by hand and from
its header, tagged and bucketed its arguments, written a profiler, and
driven tapir from the application. The guides that follow go deeper into
each topic.
