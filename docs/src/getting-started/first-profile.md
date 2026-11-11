# Your first profile

Before writing a wrapper, profile a program with one that already exists.
`examples/posixio` wraps a few C standard library functions (`fopen`,
`fclose`, `fread`, `fwrite`, `qsort`). The program
`examples/posixio/c/app.c` writes and reads some files and sorts an array
with two comparators, one exported and one `static`.

## 1. Build the wrapper and the program

```sh
cargo build --release -p tapir-posixio
gcc -O1 -rdynamic -o /tmp/app examples/posixio/c/app.c
```

`-rdynamic` exports the program's own functions, so tapir can name the
`qsort` comparator `cmp_up` instead of printing an address. More on this in
[Capture modes](../guides/capture-modes.md#sym-callbacks).

## 2. Run it under tapir

```sh
$ cd /tmp
$ ~/tapir/bin/tapir-run -v -l ~/tapir/target/release/libtapir_posixio.so \
      -p summary,bbv,trace -- ./app
[tapir] pid 170 profilers: [summary, bbv, trace]
sorted: 40..0
[tapir] wrote tapir-out/summary.p170.csv
[tapir] wrote tapir-out/bbv.p170.bb
[tapir] wrote tapir-out/bbv.blocks.p170.csv
[tapir] wrote tapir-out/trace.p170.csv
```

`tapir-run` is a short shell script. It only sets environment variables and
`exec`s the command, so this is equivalent:

```sh
TAPIR_VERBOSE=1 TAPIR_PROFILERS=summary,bbv,trace \
LD_PRELOAD=~/tapir/target/release/libtapir_posixio.so ./app
```

| `tapir-run` flag | Sets | Meaning |
|---|---|---|
| `-p PROFILERS` | `TAPIR_PROFILERS` | which profilers to run, with options |
| `-k RULES` | `TAPIR_KEY` | extra bucketing rules for every profiler |
| `-o TEMPLATE` | `TAPIR_OUT` | output path template |
| `-l LIB.so` | `LD_PRELOAD` | tapir library to preload (repeatable) |
| `-v` | `TAPIR_VERBOSE=1` | list profilers and written files on stderr |

Without `-l`, `tapir-run` preloads the GPU library built in
`target/release` (`libtapir_rocm.so`, else `libtapir_cuda.so`), or the
`:`-separated list in `$TAPIR_LIB`.

## 3. Look at the output

Every file is named `{stream}.{tag}.{ext}`, where the tag is `p<pid>`, or
`r<rank>` under MPI. Existing files are never overwritten: a second run
writes `summary.p171.csv`, and a clash gets a `.1` suffix.

### `trace`: one row per call

```text
t_ns,duration_ns,thread,depth,interval,kind,lib,function,args
13202512664,36361,0,0,0,call,stdio,fopen,"path=""./data 0.bin"" mode=""wb"""
13202556969,1719,0,0,0,call,stdio,fwrite,size=1 n=1000
13202560789,114,0,0,0,call,stdio,fwrite,size=1 n=1000
13202561362,13979,0,0,0,call,stdio,fwrite,size=4096 n=3
13202576945,5118,0,0,0,call,stdio,fclose,
```

Each row has the captured arguments. The `FILE *` and the data buffer are
not captured (the wrapper uses `skip` for them), so they do not appear.

### `summary`: calls and time per signature

```text
lib,function,signature,calls,nested_calls,total_s,mean_s,min_s,max_s
stdio,fclose,fclose(),6,0,0.000536442,0.000089407,0.000000802,0.000524984
stdio,fopen,"fopen(path=./data%200.bin,mode=wb)",2,0,0.000046062,...
stdio,fwrite,"fwrite(size=4096,n=3)",3,0,0.000024635,...
stdio,fread,"fread(size=1,n=70000)",6,0,0.000016918,...
stdio,fwrite,"fwrite(size=1,n=1000)",4,0,0.000004812,...
stdlib,qsort,"qsort(n=100,size=4,compar=cmp_up)",1,0,0.000003247,...
stdlib,qsort,"qsort(n=100,size=4,compar=app+0xb64)",1,0,0.000001528,...
```

By default `summary` keeps every argument exactly, so `fwrite(size=1,n=1000)`
and `fwrite(size=1,n=1001)` are different rows. Rows are sorted by total
time. Strings are percent-escaped in signatures (a space is `%20`). The
`static` comparator has no symbol name, so it is shown as
`object+offset`, which is the same in every run.

### `bbv`: which kinds of calls, and how many

`bbv` answers "what mix of calls did the program make?" It writes two
files, and you need both to read its output.

**Step 1: the block list (`bbv.blocks.p170.csv`).** `bbv` first sorts
calls into groups of "the same kind of call", called **blocks** (see
[Profilers, blocks and intervals](../concepts/profilers.md#blocks-when-are-two-calls-the-same)).
posixio's rules (`@bytes:log2; @count:log2`) compare sizes and counts only
by their power-of-two range. So `fwrite` calls of 1000, 1001 and 1002
items all land in one block, `n=512..1023`. Each block gets a number in
the order it was first seen. Here are all 11 blocks from this run, with
the `label` and `calls` columns from the file:

| Block | Label (what kind of call) | Calls |
|---|---|---|
| 1 | `fopen(path=./data%200.bin,mode=wb)` | 2 |
| 2 | `fwrite(size=1..1,n=512..1023)` | 6 |
| 3 | `fwrite(size=4096..8191,n=2..3)` | 3 |
| 4 | `fclose()` | 6 |
| 5 | `fopen(path=./data%200.bin,mode=rb)` | 2 |
| 6 | `fread(size=1..1,n=65536..131071)` | 6 |
| 7 | `fopen(path=./data%201.bin,mode=wb)` | 1 |
| 8 | `fopen(path=./data%201.bin,mode=rb)` | 1 |
| 9 | `qsort(n=64..127,size=4..7,compar=cmp_up)` | 1 |
| 10 | `qsort(n=64..127,size=4..7,compar=app+0xb64)` | 1 |
| 11 | `qsort(n=32..63,size=4..7,compar=cmp_up)` | 1 |

(`%20` is an escaped space in the file name. The file also has a `hash`
column: a second id for each block that stays the same across runs.)

**Step 2: the counts (`bbv.p170.bb`).** This file says how many calls of
each block happened. It holds one line:

```text
T:1:2 :2:6 :3:3 :4:6 :5:2 :6:6 :7:1 :8:1 :9:1 :10:1 :11:1
```

Read it like this:

- `T` starts a line. Each line covers one **interval** (a slice of the
  run).
- Each `:block:count` pair says "block *block* was called *count* times in
  this interval". So `:1:2` means block 1 (`fopen ... wb`) was called 2
  times, and `:2:6` means block 2 (the small `fwrite`s) was called 6
  times.
- Blocks with zero calls in an interval are left out.

Decoded, the line says: in this interval, 2 × `fopen` for writing
`data 0.bin`, 6 × small `fwrite`, 3 × 4 KiB `fwrite`, 6 × `fclose`, and so
on. That matches the `calls` column above, because nothing split the run
into intervals: the whole run is **one** interval, so one line.

**Why this format?** It is the input format of
[SimPoint](https://cseweb.ucsd.edu/~calder/simpoint/), a tool that finds
the distinct *phases* of a program. A single line, as here, is just a
total. The format becomes useful when the run is split into many
intervals, for example one per time step of a simulation. Then the file
has one line per interval, and comparing lines shows when the program's
behaviour changes. You can try it now by ending an interval every 10
calls:

```sh
tapir-run -l $LIB -p 'bbv(every=calls:10)' -- ./app
```

The tutorial's [Driving tapir from the application](../tutorial/online.md)
ends intervals at the program's own phase boundaries instead.

## 4. Try a few options

```sh
# Only fwrite and fread, as JSON lines
tapir-run -l $LIB -p 'trace(fn=fwrite;fread, format=jsonl)' -- ./app

# Coarser buckets for every profiler
tapir-run -l $LIB -p bbv -k '@bytes:log2/1; @count:drop' -- ./app

# Close a bbv interval every 10 calls
tapir-run -l $LIB -p 'bbv(every=calls:10)' -- ./app

# Write somewhere else
tapir-run -l $LIB -p summary -o 'results/{exe}.{stream}.{ext}' -- ./app
```

The rest of the book explains each of these. Next, you will write a
wrapper library of your own.
