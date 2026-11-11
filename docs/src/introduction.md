# Introduction

**tapir** (Tracing And Profiling Interposition Runtime) records the calls a
program makes into a C library: which functions it called, with which
arguments, and how long each call took. It does this **without changing or
recompiling the program**.

Some questions it can answer:

- Which BLAS or cuBLAS calls does this application make, how often, at
  what sizes, and how much time do they take?
- Does the program's use of the GPU libraries change over the run (setup,
  then a solver phase, then output)? Where are those phases?
- What does this program do with files: which ones does it open, and how
  big are its reads and writes?

tapir is a set of Rust crates, not a single finished tool. It ships ready
to use for AMD's and NVIDIA's math libraries, and gives you the pieces to
build the same thing for any other C library.

## How this book is organized

1. **Concepts** comes first. It explains, one at a time, the ideas you
   need before anything else makes sense: how a call is intercepted, how
   its arguments are described, what a profiler is, how calls are grouped
   and split over time, and how the pieces fit together. Read it in order.
   Every later chapter assumes it.
2. **Getting started** builds tapir and profiles a real program with a
   wrapper that ships with tapir. You don't write any code yet.
3. **Tutorial** is a step-by-step project. You wrap `pix`, a tiny C image
   library written for this book: first by hand, then generated
   automatically from its header. Then you write your own profiler, control
   tapir from inside the program, and analyze the results.
4. **Guides** go deeper into each component.
5. **Reference** lists every setting, option and function in one place.

## Requirements

- **Linux with glibc**, x86-64 or aarch64. tapir relies on Linux's
  dynamic linker. It does not work on macOS or Windows. On a Mac, a Linux
  container (Docker or Podman with the `rust` image) is enough to follow
  this book.
- **Rust 1.82 or newer**.
- **A C compiler** (`gcc` or `clang`) for the example programs.
- **Python 3** (optional) for reading results and running the example
  checks.

> **Conventions.** In shell examples, a `$` prompt marks commands whose
> output is shown below them. Timings, process IDs and hashes in sample
> output will differ on your machine.
