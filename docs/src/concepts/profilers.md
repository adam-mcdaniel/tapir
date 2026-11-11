# 3. Profilers, blocks and intervals

The wrapper produces a call record and hands it on. This chapter is about
what receives the records and what comes out at the end. It introduces
**profilers**, **blocks** (with the **key rules** that define them),
**intervals**, and **output files**.

## Profilers

A **profiler** is a piece of code that receives every call record, keeps
a running result in memory, and writes that result to a file at the end.
Each profiler answers one question. tapir includes three:

| Profiler | Question it answers | Output |
|---|---|---|
| `summary` | Which calls were made, how often, and how much time did they take? | a table: one row per distinct call, with count and total, mean, min and max time |
| `trace` | What exactly happened, in order? | a table: one row per call, with timestamp, duration and arguments |
| `bbv` | How does the *mix* of calls change over the run? | one vector of call counts per interval (explained below) |

You can run several at once, and you can write your own in Rust (the
tutorial does). You choose them when you run the program, with the
`TAPIR_PROFILERS` environment variable:

```sh
TAPIR_PROFILERS=summary,trace LD_PRELOAD=libtapir_posixio.so ./program
```

Options go in parentheses, for example `trace(fn=fwrite)` to trace only
`fwrite`. If you don't set `TAPIR_PROFILERS`, the wrapper library picks a
default (usually `bbv,summary`).

## Blocks: when are two calls "the same"?

`summary` and `bbv` both group calls: `summary` totals the time per
group, and `bbv` counts calls per group. So they need to decide which
calls belong together. tapir calls such a group a **block**. The name
comes from the "basic blocks" of compiled code, which phase-analysis tools
count in the same way.

The question matters more than it seems. Consider these three calls:

```text
fwrite(size=1, n=1000)
fwrite(size=1, n=1001)
fwrite(size=1, n=1002)
```

Are they one block or three?

- For an exact accounting, they are three different calls (that's
  `summary`'s default).
- For understanding the program's behaviour, they are clearly the same
  kind of work: writing about a kilobyte. Treating them as different makes
  a program look busier and more varied than it is.

**Key rules** settle this. A key rule says, for some arguments, how
coarsely to compare their values. Rules select arguments **by tag**, which
is where tags pay off. For example:

```text
@count:log2
```

means "for every argument tagged `count`, compare only the power-of-two
range". 1000, 1001 and 1002 all fall in the range 512–1023, so the three
calls above become **one block**, labelled `fwrite(size=1..1,n=512..1023)`.
The block also gets a numeric id, the same in every run.

Other ways to compare a value include: exactly (the default), rounded to a
few significant digits, in fixed-width bins, or not at all (the argument
is ignored). Some kinds of values are always handled the same way: data
addresses never count, and strings and function names are always compared
exactly.

Rules come from three places, each able to refine the one before:

1. the **wrapper library's defaults**, which its author chose to suit
   that library (`libtapir_posixio.so` uses `@bytes:log2; @count:log2`),
2. the **`TAPIR_KEY` environment variable**, which applies to every
   profiler,
3. **a single profiler's `key=` option**, for example
   `bbv(key=@count:exact)`.

## Intervals: splitting the run over time

Programs usually work in phases: read the input, set up, iterate a solver,
write output. A total over the whole run blurs these phases together. An
**interval** is a slice of the run that profilers keep separate from the
others. `bbv` produces one vector per interval: for each block, how many
calls fell into that interval. Comparing vectors shows where the phases
are. This is what SimPoint-style tools use to pick a few representative
slices of a long run.

An interval can be ended in one of three ways:

- **By the program**, at a natural boundary (the end of a time step),
  through tapir's small C API. This needs a one-line change to the
  program.
- **By the profiler**, automatically: for example `bbv(every=calls:1000)`
  ends an interval every 1000 calls.
- **Never**: the whole run is one interval. This is the default.

## Output files

At exit (and, for some profilers, at the end of each interval), each
profiler writes its results to a file in `tapir-out/`, named after the
profiler and the process:

```text
tapir-out/summary.p12345.csv       summary's table, from process 12345
tapir-out/bbv.p12345.bb            bbv's vectors, one line per interval
tapir-out/bbv.blocks.p12345.csv    bbv's list of blocks: id, label, totals
tapir-out/trace.p12345.csv         trace's rows
```

Under MPI, the rank number replaces the process id (`r0`, `r1`, ...).
tapir never overwrites an existing file. Tables can be written as CSV, TSV
or JSON lines.

## The whole flow

![Wrapped calls and calls reported with tapir_record go to the profilers. The profilers write output files at exit and at each interval. The program can optionally close intervals with tapir_interval and read the current interval with tapir_vector or tapir_snapshot.](../diagrams/data-flow.svg)

- **A.** Every wrapped call reaches every active profiler.
- **B.** Code that tapir cannot wrap can still report calls through the C
  API's `tapir_record()`. Examples are a GPU kernel seen by another tool,
  or a phase of the program you want to mark. Profilers treat these
  reports like any other call.
- **C.** Profilers write their files at exit, and at the end of each
  interval.
- **D.** The program may end intervals itself (`tapir_interval()`).
- **E.** The program may read the current interval's data while it runs
  (`tapir_vector()`, `tapir_snapshot()`), for example to store it next to
  its own results.

D and E are optional and need the program's cooperation. Everything else
works on an unmodified program.

## Summary

- A **profiler** receives every call record and produces one kind of
  result. Choose them with `TAPIR_PROFILERS`.
- A **block** is a group of calls treated as the same. **Key rules**
  select arguments by tag and say how coarsely to compare them.
- An **interval** is a slice of the run. It is ended by the program, by
  the profiler, or only at exit.
- Results go to files in `tapir-out/`.

Next: how all of this is put together, and the last two pieces,
vocabularies and `tapir-gen`.
