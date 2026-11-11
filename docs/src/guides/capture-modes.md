# Capture modes in depth

A capture mode says *how* an argument is read. Modes are mechanical: they
are the same for every C library, and the compiler rejects a mode that
does not fit the declared type.

| Mode | Reads | Accepted Rust types | Captured as |
|---|---|---|---|
| `val` | the argument itself | `i8`–`i64`, `isize`, `u8`–`u64`, `usize`, `f32`, `f64`, any pointer | `Int`, `UInt`, `Float`, or `Ptr` (address) |
| `deref` | the scalar behind a pointer | `*const T` / `*mut T` for any of the scalar types above | `Int`, `UInt`, `Float` |
| `chr` | a C `char` | `i8`, `u8` (`c_char` is one of them) | `Char` |
| `deref_chr` | the `char` behind a pointer | `*const c_char`, `*mut c_char` (or `i8`/`u8` pointers) | `Char` |
| `cstr` | a NUL-terminated string | `*const c_char`, `*mut c_char`, `*const c_void` | `Str` |
| `sym` | a code address, by symbol name | `*const c_void`, `*mut c_void`, `usize` | `Sym` |
| `skip` | nothing | anything | `None` |

A null pointer under `deref`, `deref_chr`, `cstr` or `sym` is captured as
`None` and is safe.

## `val`: scalars and enums

`val` covers integers, floats and C enums (an enum is an `int`, so declare
it as `i32`). On a pointer, `val` records the address. Addresses are shown
in `trace`, but they are **never** part of a block key, because they
change between runs.

## `skip`: handles and buffers

Use `skip` for anything whose value says nothing about the work: library
handles, streams, data buffers, output pointers. Skipping is also the
safe choice when you are not sure what a pointer points to.

## `deref`: scalars passed by pointer

Fortran and Fortran-style C bindings pass every scalar by reference:

```rust
use std::ffi::{c_char, c_void};

tapir::interpose! {
    lib "flapack";
    fn dgetrf_(m: *const i32 => deref "dim", n: *const i32 => deref "dim",
               a: *mut c_void => skip, lda: *const i32 => deref "dim",
               ipiv: *mut c_void => skip, info: *mut c_void => skip);
    fn dpotrf_(uplo: *const c_char => deref_chr "flag", n: *const i32 => deref "dim",
               a: *mut c_void => skip, lda: *const i32 => deref "dim", info: *mut c_void => skip);
}
```

The type must name the pointee (`*const i32`, not `*const c_void`) so tapir
knows how many bytes to read. With `tapir-gen`, `g.fortran(true)` makes
tagged scalar pointers `deref` and `char` pointers `deref_chr`
automatically.

> **Only dereference host memory you know is readable.** `deref`,
> `deref_chr` and `cstr` read memory *before* the real function runs. On a
> GPU library, `alpha` and `beta` may be device pointers (cuBLAS's
> `CUBLAS_POINTER_MODE_DEVICE`), and reading them from the host crashes.
> When in doubt, `skip`.

## `chr` and `deref_chr`: characters

LAPACK-style APIs pass options as characters (`'N'`, `'T'`, `'U'`). `chr`
records them as characters, so `trace` shows `trans=T` rather than `84`.
In block keys and labels, characters are stored as their code point
(`format=103` for `'g'`).

## `cstr`: strings

`cstr` copies nothing: the event borrows the application's string for the
duration of the call. Strings are always keyed exactly. In labels they are
percent-escaped (`%20` for a space, `%2C` for a comma), and in `trace` they
are quoted. Each distinct string is stored once in a table for the life of
the process once `bbv` or `summary` keys on it. Avoid `cstr` on arguments with
unbounded variety, such as generated file names in a tight loop, or drop
them from the key (`@path:drop`).

## `sym`: callbacks {#sym-callbacks}

`sym` records a code address and names it with `dladdr`:

| The code is | It is shown as |
|---|---|
| an exported symbol | `invert` |
| inside an exported symbol | `invert+0x10` |
| in an object, with no symbol (a `static` function) | `app+0xbe0` (file name + offset from its load base) |
| not in any loaded object | the raw address |

All of these stay the same across runs, even with address-space
randomization. Link executables with `-rdynamic` so their non-static
functions have names. Declare function-pointer parameters as
`*const c_void`.

## Types that cannot be wrapped

- **Variadic functions** (`printf`, `open`, `ioctl`): `interpose!` cannot
  forward a `...` argument list. `tapir-gen` skips them.
- **Structs passed by value**, including `cuComplex`-style complex
  numbers: their ABI depends on the struct's layout, so they are skipped.
  Structs passed by pointer are fine (`skip` them).
- **`long double`, bit-fields, unions by value**: not supported.

To capture something these modes cannot express (a field inside a struct,
say), write the `extern "C"` wrapper yourself and call `tapir::call` with
your own values. See
[Events from other sources](other-sources.md#instrumenting-rust-code-with-tapircall).
