# 2. Describing arguments: modes and tags

A wrapper records the arguments of each call. But a C function's arguments
are just bytes: an `int`, a pointer, a `char *`. To make use of them,
every argument needs two separate pieces of information, and tapir keeps
them separate on purpose:

| | Question it answers | Who decides | Example |
|---|---|---|---|
| **Mode** | *How* do I read this value from memory? | follows from the C type | "it's a number", "it's a string", "don't read it" |
| **Tag** | *What does* this value mean? | the wrapper's author, freely | "it's a byte count", "it's a file path" |

This chapter explains both and why both are needed.

## Modes: how to read a value

A **capture mode** is a mechanical instruction for reading one argument.
There are seven:

| Mode | Reads | Use it for | Example |
|---|---|---|---|
| `val` | the value itself | numbers and enums | `size_t n` → `1000` |
| `cstr` | the text a `char *` points to | strings | `const char *path` → `"data.bin"` |
| `chr` | a single character | a `char` argument | `char trans` → `T` |
| `deref` | the number a pointer points to | numbers passed by pointer (Fortran style) | `const int *n` → `512` |
| `deref_chr` | the character a pointer points to | characters passed by pointer | `const char *uplo` → `U` |
| `sym` | the name of the function a pointer points to | callbacks | `qsort`'s comparator → `cmp_up` |
| `skip` | nothing | handles, data buffers, anything that doesn't matter | `FILE *f` |

The mode is dictated by the C type: a `size_t` can only be read with
`val`, and a callback only makes sense with `sym`. The Rust compiler
rejects a mode that doesn't fit the declared type, so you cannot, for
example, read an integer as a string by mistake.

Why skip anything? Some values change from run to run without meaning
anything: the address of a buffer, a `FILE *`, a library handle. Recording
them would make two identical runs look different. Others, such as the
contents of a data buffer, are too large or not safe to read.

## Tags: what a value means

The mode tells tapir that `fwrite`'s second argument is the number 1. It
does not tell tapir that this number is a *size in bytes*, or that the
third argument is a *count of items*. That meaning is what a **tag**
carries.

A tag is any short string you choose: `"bytes"`, `"count"`, `"path"`,
`"size"`, `"rank"`. tapir never interprets it. It is a label that other
parts of tapir can select on. Tags are optional: an argument without a
tag is still recorded.

Here are the wrappers in `libtapir_posixio.so`, written out as a table:

| Function | Argument | C type | Mode | Tag |
|---|---|---|---|---|
| `fopen` | `path` | `const char *` | `cstr` | `path` |
| `fwrite` | `ptr` | `const void *` | `skip` | |
| `fwrite` | `size` | `size_t` | `val` | `bytes` |
| `fwrite` | `n` | `size_t` | `val` | `count` |
| `fwrite` | `f` | `FILE *` | `skip` | |
| `qsort` | `base` | `void *` | `skip` | |
| `qsort` | `n` | `size_t` | `val` | `count` |
| `qsort` | `size` | `size_t` | `val` | `bytes` |
| `qsort` | `compar` | function pointer | `sym` | |

In Rust, a row of this table is written `size: usize => val "bytes"`:
the argument name, its type, then the mode, then the tag.

Notice two things:

- `size` and `n` have the **same mode** (`val`) but **different tags**
  (`bytes` and `count`). They are read the same way but mean different
  things.
- `fwrite`'s `size` and `qsort`'s `size` are in **different functions**
  but have the **same tag**. They mean the same kind of thing.

## Why tags matter

Tags let the rest of tapir work across functions and libraries without
knowing any of them. Instead of saying "for `fwrite`, the second argument,
and for `qsort`, the third argument, and for `fread`, ...", you say
"**every argument tagged `bytes`**", once.

You will see two uses in the next chapter:

- **Grouping rules** decide which calls count as "the same kind of call",
  for example "round every `bytes` value to a power of two".
- **Profilers** read values by tag, for example "add up every argument
  tagged `bytes`".

A rule or profiler written for the tag `bytes` works for every wrapper
that uses that tag, including libraries that didn't exist when it was
written.

## Don't confuse tags with library labels

Each wrapper also belongs to a **library label**, such as `stdio` or
`cuda`. That is a name for a whole group of functions, used in output and
for filtering ("only `cuda` calls"). It is not an argument tag.

## Summary

- Every recorded argument has a **mode** (how to read it, set by its C
  type) and optionally a **tag** (what it means, chosen by you).
- Modes make recording safe and mechanical. Tags make the recorded values
  meaningful, so rules and profilers can work across many functions at
  once.

Next: what happens to the recorded calls.
