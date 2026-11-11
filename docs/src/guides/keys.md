# Block keys and bucketing

A **key policy** decides which calls count as "the same thing". `bbv`
uses it to define blocks, `summary(key=...)` to define rows, and your own
profilers can use it through `KeyPolicy`.

A key is the function plus, for each captured argument, a *bucketed*
value. Two calls with equal keys are the same block.

## Rule syntax

Rules are `selector:bucket`, separated by `;`:

```text
@size:log2; @radius:drop; pix_resize.filter:drop
```

| Selector | Applies to |
|---|---|
| `*` | every argument |
| `@tag` | arguments with this tag |
| `@` | untagged arguments |
| `name` | arguments with this parameter name, in any function |
| `fn.name` | parameter `name` of functions matching the glob `fn` |
| `fn.@tag`, `fn.@`, `fn.*` | likewise, only within functions matching `fn` |

## Buckets

| Bucket | Groups values by | Example values → label |
|---|---|---|
| `exact` | the exact value (the default) | 1000 → `1000` |
| `log2` (or `bits`) | bit length; bucket *b* holds [2^(b-1), 2^b) | 1000 → `512..1023`, 1 → `1..1`, 0 → `0` (the sign is kept) |
| `log2/N` | bit length with 2^N sub-buckets per power of two | `log2/2`: 1000 → `~2^9.75` |
| `sig/N` | rounded to *N* significant bits | `sig/3`: 1000 → `~1024`, 700 → `~640`, 111 → `~112` |
| `linear/W` | fixed-width bins of width *W* | `linear/100`: 1000 → `1000..1099` |
| `zeroone` (or `01`) | 0, 1, -1, or other | flags, `alpha`/`beta`, "is it zero" |
| `drop` (or `ignore`, `none`) | ignore the argument | scalars, data-dependent values |

Whatever the bucket:

- **Pointers** (`val` on a pointer) never enter a key.
- **Strings** and **symbols** are always exact.
- **Characters** are keyed by code point.
- **Floats**: `exact` uses the bit pattern, `zeroone` works as expected,
  and every other bucket uses the sign and binary exponent. Their labels
  are not meaningful, so prefer `drop` or `zeroone` for floats.

## Which rule wins

For each argument, every matching rule gets a score:

1. A function-qualified rule (`fn.…`) beats an unqualified one.
2. Within that, a parameter name beats a tag, and a tag beats `*`.
3. Among rules with equal scores, the **last** one wins.

With no matching rule, the argument is kept exactly. An example:

```text
@size:log2; n:exact; resize.w:drop; img_*.@size:sig/3; *:drop
```

| Function | Argument (tag) | Bucket | Why |
|---|---|---|---|
| `resize` | `n` (`size`) | `exact` | name beats tag |
| `resize` | `w` (`size`) | `drop` | function-qualified name |
| `crop` | `w` (`size`) | `log2` | tag rule |
| `img_blur` | `w` (`size`) | `sig/3` | function-qualified tag beats unqualified |
| `x` | `mode` (none) | `drop` | only `*` matches |

## Where rules come from

Rules are concatenated in this order and then resolved as described
above:

1. The wrapper library's `b.default_key(...)` (repeated calls append).
2. `TAPIR_KEY`, or `tapir-run -k`.
3. The profiler's `key=` option.

`bbv(key=...)` adds to 1 and 2. `summary(key=...)` starts from `exact`
unless you write `key=default`. `key=exact` and `key=name` are shortcuts
that ignore 1 and 2.

Because ties go to the later rule, a user can always override a library
default of the same specificity:

```sh
TAPIR_KEY='@size:exact'          # undo the library's @size:log2
TAPIR_KEY='@size:sig/3'          # or choose a different granularity
```

To override a *more* specific library rule (`pix_resize.width:log2`),
write a rule that is at least as specific.

## Choosing granularity

- **Too fine** (all exact): every slightly different size is its own
  block, so vectors are sparse and similar phases look different.
- **Too coarse** (`key=name`): phases that do very different amounts of
  work look the same.
- **A good start**: `log2` on sizes, `exact` on modes and flags, `drop` on
  scalars. That is the `linalg` vocabulary's default
  (`@dim:log2; @count:log2; @scalar:drop`).
- **Not sure?** Record with `bbv(key=exact)` and re-bucket offline with
  `tapir_read.rebucket` (see [Reading the output](../tutorial/reading-output.md#re-bucketing-offline)).

## Using keys in Rust

```rust
use tapir::{BlockTable, KeyPolicy, Bucket};

let mut policy = KeyPolicy::parse("@size:log2; @radius:drop", KeyPolicy::exact())?;
policy = policy.with("pix_resize.filter", Bucket::Drop)?;

let mut blocks = BlockTable::default();
let mut scratch = Vec::new();
// in on_event:
policy.key_into(call, &mut scratch);                 // bucketed key
let id = blocks.intern(call.spec, &scratch);         // dense id, first-seen order
// at output time:
let (spec, key) = blocks.entry(id);
let label = policy.describe(spec, key);              // "pix_resize(width=512..1023,height=512..1023)"
```

`KeyPolicy::exact()` keeps everything and `KeyPolicy::name_only()` drops
everything. `policy.bucket_for(func, arg)` tells you which bucket an
argument gets.
