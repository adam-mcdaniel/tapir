# tapir-gen

## Command line

```text
tapir-gen [options] HEADER... > wrappers.rs
```

Options apply to the headers that follow them.

| Option | Effect |
|---|---|
| `--lib NAME` | library tag for the following headers (default: the header's file stem) |
| `--export MACRO` | only take prototypes that start with `MACRO` (repeatable); without it, every top-level prototype is taken |
| `--marker MACRO` | like `--export`, but `MACRO` may stand anywhere in the prototype (`ret MACRO name(...)`); repeatable |
| `--ignore WORD` | drop `WORD` from prototypes (calling-convention macros) |
| `--scan-types` | register the handles, enums and scalar aliases each following header declares with `typedef` |
| `--type NAME=KIND` | add a type: `KIND` is a Rust scalar (`i32`, `f64`, ...), `enum` or `handle` |
| `--arg SELECTOR=ACTION` | a parameter rule, e.g. `n=@size`, `path=cstr@path`, `myfn.flags=skip` |
| `--rules FILE` | parameter rules from a file (`selector action` lines, `#` comments) |
| `--include GLOB` | only wrap matching function names (repeatable) |
| `--exclude GLOB` | never wrap matching function names (repeatable) |
| `--fortran` | scalars passed by reference |
| `--report` | print the wrapped/skipped report to stderr |

## Library API

```rust
let mut types = tapir_gen::TypeMap::c_basic();
types.handle("h_t").enumeration("e_t").scalar("lib_int", "i32");
types.scan(&header_text);                       // or discover typedefs

let mut g = tapir_gen::Generator::new(types);
g.rules.parse_text(rules_text)?;                // "selector action" lines
g.rules.add("lib_open.name", "cstr@path")?;     // one rule
g.include("lib_*").exclude("*_create*");        // name globs
g.ignore("LIBAPI_CALL");                        // drop a word from prototypes
g.fortran(true);                                // by-reference scalars

g.header(path, "lib", &["LIB_EXPORT"])?;        // export macro first: `LIB_EXPORT ret name(...)`
g.header(path, "lib", &[])?;                    // every top-level prototype
g.header_marked(path, "lib", &["LIB_API"])?;    // macro anywhere: `ret LIB_API name(...)`
g.source(&text, "lib", &[...]);                 // same, from a string
g.source_marked(&text, "lib", &[...]);

let code: String = g.render();                  // one interpose! block, in a private module
let report: String = g.report();                // wrapped/skipped summary
for f in g.functions() { /* f.lib, f.name, f.ret, f.params[i].{name, ffi, mode, tag} */ }
```

When the same function name appears twice, the first declaration wins.

## Rules

```text
selector   action
```

| Selector | Matches |
|---|---|
| `name` | a parameter name (glob, case-insensitive) |
| `func.name` | that parameter in functions matching `func` (glob) |
| `:int`, `:float`, `:char`, `:enum`, `:handle`, `:ptr` | parameters of that C kind |
| `func.:kind` | that kind within matching functions |

| Action | Meaning |
|---|---|
| `@tag` | default mode, with this tag |
| `mode` | this mode, no tag |
| `mode@tag` | both |

Modes: `val`, `deref`, `chr`, `deref_chr`, `cstr`, `sym`, `skip`.

Precedence: any rule that matches the parameter **name** beats any kind
rule. Among name rules, and among kind rules, the **later** rule wins.
There are no built-in rules.

## Defaults and allowed modes

| C parameter | FFI type | Default mode | Allowed modes |
|---|---|---|---|
| by-value integer or float (known scalar) | the mapped Rust type | `val` | `val`, `skip` |
| `char` | `c_char` | `chr` | `chr`, `val`, `skip` |
| enum | `i32` | `val` | `val`, `skip` |
| handle | `*mut c_void` | `skip` | `skip`, `val`, `sym` |
| pointer | `*const c_void` / `*mut c_void` | `skip` | `skip`, `val`, `sym`; `deref` (pointer to a known scalar, enum or `char`); `cstr`/`deref_chr` (pointer to `char`, `int8_t`, `uint8_t`) |
| pointer, with `fortran(true)` | as above | `deref` if tagged and pointing to a scalar or enum; `deref_chr` for `char *` | as above |
| unknown by-value type | | *function skipped* | |
| `...` | | *function skipped* | |

A pointer is `*const` when the declaration starts with `const` and has one
level of indirection, otherwise `*mut`. Parameter names that are Rust
keywords get a trailing underscore (`fn` → `fn_`). Rules still match the C
name.

`TypeMap::c_basic()` knows `int`, `signed`, `unsigned [int]`,
`long [int]`, `unsigned long`, `long long [int]`, `unsigned long long [int]`,
`signed char`, `unsigned char`, `bool`, `_Bool`, `short`, `unsigned short`,
`int8_t`–`int64_t`, `uint8_t`–`uint64_t`, `size_t`, `ssize_t`, `ptrdiff_t`,
`float`, `double` and `char`.

## What the parser handles

It removes comments, preprocessor lines and string literals, splits the
text into statements at `;`, `{` and `}`, and reads `ret name(params)`
from each. It ignores `typedef`, `static` and `inline` declarations, drops
`extern`, `__attribute__((...))`, `[[...]]` and `*_DEPRECATED` words from
return types, and skips a macro call in front of a marked prototype
(`LIB_DEPRECATED(new) ret LIB_API name(...)`). It does not run the
preprocessor, so prototypes produced by macros are not seen.
