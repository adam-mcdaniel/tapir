# The built-in profilers

Select profilers with `TAPIR_PROFILERS` (or `tapir-run -p`). Separate
profilers with commas, and put options in parentheses:

```sh
TAPIR_PROFILERS='bbv(weight=time, every=calls:5000), summary(key=default), trace(fn=pix_*)'
```

Inside parentheses, options are `key=value` separated by commas. A list
inside one value uses `;` (`lib=pix;stdio`). A bare `flag` means
`flag=` (true).

## Options every profiler understands

| Option | Effect |
|---|---|
| `as=NAME` | run another instance under a different name: `bbv, bbv(as=bbv_time, weight=time)` |
| `format=csv\|tsv\|jsonl` | row format (default from `TAPIR_FORMAT`, else `csv`) |

The built-ins also share a call filter:

| Option | Effect |
|---|---|
| `lib=a;b` | only calls whose library tag is listed |
| `fn=glob;glob` | only functions matching one of the globs (`*`, `?`) |
| `nested=0\|1` | include calls made from inside another wrapped call |

`nested` defaults to **off** for `bbv` (so a library calling another
wrapped library is not counted twice) and **on** for `summary` and `trace`.

## `bbv`: basic-block vectors over calls

Each distinct bucketed call is a "basic block", and each interval yields a
sparse vector of block → weight. This is the input SimPoint-style phase
analysis needs.

| Option | Default | Effect |
|---|---|---|
| `key=` | library default | rules *added to* the library's default and `TAPIR_KEY`; `key=exact` ignores them all and keeps every signature; `key=name` keys on function name only |
| `weight=calls\|time` | `calls` | each call adds 1, or its duration in ns |
| `every=calls:N` | none | close an interval every *N* recorded calls |
| `every=time:S` | none | close an interval once *S* seconds have passed (checked at each call) |
| `format=simpoint\|csv\|tsv\|jsonl` | `simpoint` | `.bb` lines, or a long `interval,block,hash,weight` table (`TAPIR_FORMAT` does not change this default) |

Streams: `bbv` (one vector per interval) and `bbv.blocks` (the block
table).

Intervals:

- **External** (no `every=`): closed by `tapir_interval()` or by
  `tapir_vector("bbv", 1, ...)`. Empty intervals still produce a line, so
  line *N* is always interval *N*.
- **`every=`**: `bbv` closes its own intervals and ignores
  `tapir_interval()`.
- At exit, the open interval is written if it has any calls, or if no
  interval was ever closed.

Online access: `tapir_snapshot("bbv", ...)` returns the open interval as a
`.bb` line, and `tapir_vector("bbv", ...)` returns `(hash, weight)` pairs.

## `summary`: totals per signature

Calls, nested calls, and total, mean, minimum and maximum duration per
signature, sorted by total time.

| Option | Default | Effect |
|---|---|---|
| `key=exact\|default\|name\|RULES` | `exact` | `exact`: every distinct signature; `default`: the library's default rules plus `TAPIR_KEY`; `name`: per function; `RULES`: these rules on top of `exact` |
| `every=N` | 0 | also write `summary.partial<index>` every *N* intervals |

Note that `summary(key=RULES)` starts from exact, while
`bbv(key=RULES)` starts from the library's defaults.

## `trace`: one row per event

Every call and marker, with time, duration, thread, depth, interval,
library, function and arguments.

| Option | Default | Effect |
|---|---|---|
| `batch=N` | 4096 | rows buffered before a write (also flushed at every interval and at exit) |

A trace grows with the number of calls. Use `fn=` and `lib=` to limit it.

## Overhead

Per-call cost is roughly 110 ns with `bbv`, and about 25 ns with
recording paused. Most of it is reading the clock. Profilers that never
read call times return `false` from `needs_time()`; if *every* active
profiler does, tapir skips the clock entirely. `bbv` with `weight=calls`
and no `every=time:` is one of them.
