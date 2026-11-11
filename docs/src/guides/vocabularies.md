# Vocabularies

A tag only means something if the wrapper and the key rules agree on it.
When you wrap a *family* of libraries (rocBLAS and cuBLAS, or several I/O
libraries), you want the same parameter names to get the same tags
everywhere, and the same default bucketing. A **vocabulary** is that
shared knowledge, kept as plain data in `crates/tapir-vocab`.

A vocabulary has two parts:

- **`args`**: `tapir-gen` rules (`selector action` lines) that tag
  parameters by name and C kind.
- **`key`**: the default key rules for the runtime.

```rust
pub struct Vocab {
    pub name: &'static str,
    pub args: &'static str,   // tapir-gen parameter rules
    pub key: &'static str,    // default KeyPolicy rules
}
```

## The `linalg` vocabulary

`tapir_vocab::LINALG` covers BLAS, LAPACK and similar APIs, and is used by
`tapir-rocm`, `tapir-cuda` and `examples/fakeblas`.

| Tag | For | Default bucket |
|---|---|---|
| `dim` | problem sizes and leading dimensions (`m`, `n`, `k`, `lda`, `nnz`, ...) | `log2` |
| `count` | repetition counts (`batch_count`, `max_sweeps`) | `log2` |
| `stride` | increments, strides, offsets (`incx`, `stride_a`) | exact |
| `flag` | modes and options (`trans`, `uplo`, `side`, `job*`, any enum or `char`) | exact |
| `scalar` | numeric scalars by value (`alpha`, tolerances, any float) | `drop` |

Its rules file starts with kind rules and then names parameters:

```text
:enum   @flag
:char   @flag
:float  @scalar

m       @dim
n       @dim
k       @dim
ld*     @dim
inc*    @stride
trans*  @flag
alpha   @scalar
...
```

and its key is `@dim:log2; @count:log2; @scalar:drop`.

## Using a vocabulary

In the wrapper's `build.rs`, load the rules first, then your own, so that
yours win:

```rust
let mut g = tapir_gen::Generator::new(types);
g.rules.parse_text(tapir_vocab::LINALG.args)?;   // shared rules
g.rules.parse_text(MY_EXTRA_RULES)?;             // library-specific additions win
```

In the wrapper's setup, use its key:

```rust
tapir::preload!(|b| {
    b.default_key(tapir_vocab::LINALG.key);
    b.default_key("@mylib_tag:log2");     // optional additions; repeated calls append
    b.from_env("bbv,summary");
});
```

From the command line: `tapir-gen --rules crates/tapir-vocab/src/linalg.args ...`.

Remember to add `tapir-vocab` to both `[dependencies]` (for `.key`) and
`[build-dependencies]` (for `.args`).

## Adding a vocabulary

1. Write `crates/tapir-vocab/src/<name>.args`. Start with kind rules
   (`:enum`, `:float`, ...), then parameter names. Name rules always beat
   kind rules, and among name rules the later line wins, so put general
   globs before specific names.
2. Add a constant next to `LINALG` in `crates/tapir-vocab/src/lib.rs`:

   ```rust
   /// POSIX-style I/O: byte counts bucketed, paths and flags exact.
   pub const IO: Vocab = Vocab {
       name: "io",
       args: include_str!("io.args"),
       key: "@bytes:log2; @count:log2",
   };
   ```

3. Add it to `ALL`, so `tapir_vocab::get("io")` finds it.

Keep the tags generic (`bytes`, `path`, `flag`), not specific to one
library. That way one key rule, one profiler and one offline script work
across the whole family.
