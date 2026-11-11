# MPI, several libraries, child processes

## MPI

tapir needs nothing special under MPI. Put `tapir-run` after the launcher,
so each rank runs it:

```sh
srun -n 64 tapir/bin/tapir-run -p bbv -- ./app
mpirun -np 8 tapir/bin/tapir-run -p bbv,summary -- ./app
```

Each process detects its rank from the first variable that is set, in
this order: `TAPIR_RANK`, `OMPI_COMM_WORLD_RANK`, `PMI_RANK`, `PMIX_RANK`,
`SLURM_PROCID`, `MV2_COMM_WORLD_RANK`, `ALPS_APP_PE`. Files are then named
with `r<rank>` instead of `p<pid>` (`bbv.r12.bb`). Set `TAPIR_RANK` to
override the detected rank.

`bbv` block hashes depend only on the label, so they are identical across
ranks. You can merge vectors from all ranks by hash, without a shared
block table.

## Several tapir libraries at once

Each tapir wrapper library is self-contained, so you can preload several:

```sh
tapir-run -l libtapir_pix.so -l libtapir_posixio.so -p bbv,summary -- ./app
```

- **Each library has its own runtime** with its own profilers, and sees
  only its own wrapped functions. Each one reads `TAPIR_PROFILERS` and
  writes its own files. With identically named streams, the second library
  to write gets a `.1` suffix. A library that saw no calls writes nothing.
  A custom profiler that only one library registers makes the other print
  `[tapir] error: unknown profiler ...`, which is harmless.
- **The C ABI reaches all of them.** Every library exports `tapir_interval`
  and the other ABI functions. The application's call binds to the first
  one, which handles its own runtime and forwards the call to the next
  (`dlsym(RTLD_NEXT)`). One `tapir_try_interval(-1)` therefore closes the
  interval everywhere.
- **Nested calls** across libraries are tracked per thread. If a wrapped
  rocSOLVER function calls a wrapped rocBLAS function, the inner call has
  `depth > 0` and is left out of `bbv` by default. This works within a
  single tapir library; separate libraries each count depth for their own
  calls only.

If you control both wrappers, one library that wraps both APIs (two
`lib "..."` sections) gives one runtime, combined vectors, and correct
nesting.

## Child processes

`LD_PRELOAD` is inherited, so every program the application starts loads
the wrapper library too. This is usually harmless:

- A process that never records an event writes nothing at exit. That
  covers shells, `sh -c`, compilers and other helpers.
- A child that *does* call the wrapped library is profiled as its own
  process, with its own PID-tagged files.

To keep children out entirely, the application can remove `LD_PRELOAD`
from the environment it passes to them.

## Threads

Wrappers can be called from any thread. Each thread gets a small index
(`Call.thread`, the `thread` column in `trace`), in first-call order. Each
profiler sits behind its own mutex, so profilers serialize between
threads, but the real functions run concurrently. The clock is
`CLOCK_MONOTONIC`, so timestamps are comparable across threads, and with
other tools that use the same clock.
