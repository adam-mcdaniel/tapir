# 4. How the pieces fit together

The first three chapters followed one call from the program to an output
file. This chapter introduces the last three pieces, which all help
*build* wrapper libraries or *use* them from a program: **tapir-gen**,
**vocabularies**, and the **C API**. Then it shows how everything
relates.

## tapir-gen: generating wrappers

A wrapper declaration lists a function's parameters with a mode and tag
for each. Writing these by hand works for a few functions. cuBLAS alone
has thousands.

**tapir-gen** reads a C header and writes the declarations for you:

- It turns each C parameter type into the matching Rust type.
- It picks a **mode** from the type: numbers get `val`, a `char` gets
  `chr`, and pointers and handles get `skip`.
- It picks a **tag** from **rules** you give it, matched against
  parameter names. For example, the rule `ld* @dim` tags every parameter
  whose name starts with `ld` as `dim`. Rules can also override the mode,
  for example `path cstr@path` to read `path` as a string.
- It skips, and reports, any function it cannot wrap safely (see
  [Which functions get wrapped](intercepting.md#which-functions-get-wrapped)).

It runs either as a command-line tool or as part of the wrapper crate's
build (in `build.rs`). In the second case, the wrappers are regenerated
from whatever headers are installed when you build.

## Vocabularies: sharing tags across libraries

Tags and key rules only work across libraries if the libraries agree on
them. If cuBLAS's wrapper tags matrix sizes `dim` but rocBLAS's tags them
`size`, a key rule like `@dim:log2` covers only one of them.

A **vocabulary** is a written-down agreement for one family of
libraries. It has two parts:

1. **Tag rules for tapir-gen**: which parameter names get which tags
   (`m`, `n`, `k`, `lda` → `dim`; `trans`, `uplo` → `flag`;
   `alpha`, `beta` → `scalar`; ...).
2. **Default key rules** for the wrapper library: how those tags are
   compared (`@dim:log2; @count:log2; @scalar:drop`, meaning sizes by
   power of two and scalars ignored).

tapir currently has one vocabulary, **`linalg`**, for BLAS, LAPACK and
similar libraries. The CUDA and ROCm wrapper libraries both use it, so
their profiles look alike and the same analysis works on both.
Vocabularies are optional. A wrapper for one small library, like the
tutorial's, can simply choose its own tags.

## The C API: talking to tapir from the program

Everything so far works on an unmodified program. Some uses need the
program's help. tapir provides a small C API (the header
`include/tapir.h`), and a program can:

- **end an interval** at a natural boundary (`tapir_interval`),
- **pause and resume** recording, for example to skip setup
  (`tapir_enable`),
- **read the current interval's data** while running (`tapir_vector`,
  `tapir_snapshot`),
- **report work tapir cannot wrap** as if it were a call (`tapir_record`),
- **mark the start and end of a region** (`tapir_marker`).

The header's helper functions find tapir at run time and do nothing if no
wrapper library is loaded, so the same program binary still runs normally
without tapir.

## The whole picture

![Diagram in two parts. Built once: a C header and optionally a vocabulary feed tapir-gen, which generates wrapper declarations (or you write them by hand); the vocabulary's key rules feed the setup; declarations and setup make up the wrapper library. Every run: LD_PRELOAD loads the wrapper library; the unchanged program calls a wrapper; the wrapper forwards the call to the real library and builds a call record; the record goes to the profilers, configured by your settings; profilers write output files at exit or at each interval.](../diagrams/pieces.svg)

**Built once** (by whoever makes the wrapper library):

1. Start from the C library's **header**.
2. Decide **which functions** to wrap and, for each argument, its **mode**
   and **tag**. Either write the declarations by hand, or let
   **tapir-gen** generate them using tag rules (often from a
   **vocabulary**).
3. Write a short **setup**: the library's default key rules (often the
   vocabulary's) and its default profilers.
4. Build it all into a **wrapper library** (`.so`). It also contains
   tapir's runtime.

**Every run** (by whoever profiles a program):

1. Run the program with the wrapper library in `LD_PRELOAD`, and choose
   profilers and rules with environment variables.
2. Each wrapped call goes through a **wrapper**, which forwards it to the
   **real library** and builds a **call record**.
3. The **profilers** receive every record, group the calls into **blocks**
   using **key rules**, and keep separate results per **interval**.
4. At exit (and at each interval), the profilers write **output files**.

## How tapir's code is split up

The repository's crates follow the same lines:

| Crate | Contains | Knows about specific libraries? |
|---|---|---|
| `crates/tapir` | the runtime, the `interpose!` and `preload!` macros, modes, key rules, the built-in profilers, the C API | no, only modes and tags as opaque strings |
| `crates/tapir-gen` | the header reader and generator | no |
| `crates/tapir-vocab` | vocabularies (currently `linalg`) | one *family* of libraries |
| `crates/tapir-cuda`, `crates/tapir-rocm`, `examples/*` | wrapper libraries | one library each |

## Which parts do you need?

| You want to... | You need | Start at |
|---|---|---|
| profile a program with an existing wrapper library | profilers, key rules, intervals, output files | [Your first profile](../getting-started/first-profile.md) |
| wrap a small library | the above, plus modes, tags, `interpose!` | [The tutorial](../tutorial/pix.md) |
| wrap a large library | the above, plus `tapir-gen`, maybe a vocabulary | [Generating wrappers](../tutorial/generating.md) |
| compute something tapir doesn't | writing a profiler | [Writing a profiler](../tutorial/custom-profiler.md) |
| align results with your program's phases | the C API | [Driving tapir from the application](../tutorial/online.md) |

## Glossary

| Term | Meaning |
|---|---|
| **Wrapper** | a stand-in for one C function that calls the real one and records the call |
| **Wrapper library** | a `.so` built with tapir that holds the wrappers for one C library; loaded with `LD_PRELOAD` |
| **Interposition** | getting a program's calls to go through your function first, here via `LD_PRELOAD` |
| **Runtime** | the part of tapir inside every wrapper library that runs the profilers |
| **Library label** | a name for a group of wrapped functions (`stdio`, `cuda`), used in output and filters |
| **Mode** (capture mode) | *how* to read an argument: `val`, `cstr`, `chr`, `deref`, `deref_chr`, `sym`, `skip` |
| **Tag** | a free-form label saying *what an argument means* (`bytes`, `dim`, `path`) |
| **Call record** | what a wrapper produces per call: function, arguments, times, depth, thread |
| **Nested call** | a wrapped call made from inside another wrapped call (depth > 0) |
| **Profiler** | code that receives every call record and produces one kind of result |
| **Block** | a group of calls counted as "the same" |
| **Key rules** | rules saying how coarsely to compare argument values, by tag or name, when forming blocks |
| **Interval** | a slice of the run that profilers keep separate |
| **Vocabulary** | shared tag rules and key rules for a family of libraries (e.g. `linalg`) |
| **tapir-gen** | the tool that generates wrapper declarations from C headers |
| **C API** | functions in `tapir.h` a program can call to control tapir or report work |
