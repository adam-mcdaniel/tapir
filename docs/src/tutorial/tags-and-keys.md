# Tags and block keys

So far every distinct argument combination is a separate entry:
`pix_resize(width=1000,height=1001)` and `pix_resize(width=1000,height=1002)`
are different rows. For a summary that is often what you want. For phase
analysis it is noise: those two calls do the same work.

tapir solves this in two parts:

1. The **wrapper** gives arguments *tags*, which say what they mean.
2. **Key rules** say, per tag, how to bucket values when deciding whether
   two calls are "the same block".

## Step 1: tag the arguments

A tag is any string, written after the mode. tapir never interprets it.
Pick names that describe the role of the argument in *your* API:

```rust
use std::ffi::{c_char, c_void};

tapir::interpose! {
    lib "pix";
    fn pix_load(path: *const c_char => cstr "path") -> *mut c_void;
    fn pix_save(img: *mut c_void => skip, path: *const c_char => cstr "path") -> i32;
    fn pix_resize(img: *mut c_void => skip, width: i32 => val "size", height: i32 => val "size",
                  filter: i32 => val "mode") -> i32;
    fn pix_blur(img: *mut c_void => skip, radius: f32 => val "radius") -> i32;
    fn pix_convert(img: *mut c_void => skip, format: c_char => chr "mode") -> i32;
    fn pix_map(img: *mut c_void => skip, f: *const c_void => sym) -> i32;
    fn pix_free(img: *mut c_void => skip);
}
```

`width` and `height` share the tag `size`, so one rule covers both.
Untagged arguments (`f` here) are fine. They are kept exactly unless a rule
says otherwise.

## Step 2: give the library a default key

```rust
tapir::preload!(|b| {
    // How this library's calls are grouped into blocks by default.
    b.default_key("@size:log2; @radius:drop");
    // Profilers: TAPIR_PROFILERS if set, otherwise these.
    b.from_env("bbv,summary");
});
```

Each rule is `selector:bucket`, and rules are separated by `;`:

- `@size:log2` buckets arguments tagged `size` by bit length (power-of-two
  classes).
- `@radius:drop` leaves the blur radius out of the key entirely.

Everything else is kept exactly. Pointers never take part in a key (their
addresses change between runs), and strings and symbols are always
compared exactly.

## Step 3: look at the blocks

Rebuild and run with the default profilers:

```sh
$ LD_PRELOAD=../tapir-pix/target/release/libtapir_pix.so ./app
$ cat tapir-out/bbv.blocks.*.csv
block,hash,lib,function,label,args,calls,weight
1,660367942422571909,pix,pix_load,pix_load(path=cat.png),path@path=cat.png,2,2
2,1332056758369176999,pix,pix_resize,"pix_resize(width=128..255,height=128..255,filter=1)",width@size=128..255 height@size=128..255 filter@mode=1,3,3
3,5913847550531565105,pix,pix_blur,pix_blur(),,6,6
4,7859001936488495497,pix,pix_convert,pix_convert(format=103),format@mode=103,6,6
5,6910843680588863862,pix,pix_map,pix_map(f=app+0xbe0),f=app+0xbe0,4,4
6,2842538423461099756,pix,pix_save,pix_save(path=out.png),path@path=out.png,6,6
7,10834788597726994206,pix,pix_free,pix_free(),,6,6
8,16657638241613583855,pix,pix_load,pix_load(path=dog.png),path@path=dog.png,2,2
9,9272557343710483103,pix,pix_map,pix_map(f=invert),f=invert,2,2
10,8383410772439208238,pix,pix_load,pix_load(path=bird.png),path@path=bird.png,2,2
11,4344425544286629583,pix,pix_resize,"pix_resize(width=512..1023,height=512..1023,filter=1)",width@size=512..1023 height@size=512..1023 filter@mode=1,3,3
```

The six resizes now fall into two blocks, one per phase: `128..255` and
`512..1023`. All six blurs are a single block, because the radius was
dropped. Each block has two ids:

- **`block`**: dense and 1-based, in first-seen order. This is what the
  SimPoint `.bb` file uses.
- **`hash`**: a 64-bit FNV-1a hash of the label. It is the same across
  runs and MPI ranks, so it is the id to use when comparing profiles.

In a key, a character is stored by its code point (`format=103` is
`'g'`). `trace` still prints it as `g`.

## Step 4: experiment without rebuilding

The library's default is only a starting point. Users can add rules at run
time. Rules are collected in this order:

1. the wrapper's `b.default_key(...)`,
2. `TAPIR_KEY` (applies to every profiler),
3. a profiler's own `key=` option.

A more specific rule always wins (a parameter name beats a tag). Between
equally specific rules, the later one wins, so `TAPIR_KEY='@size:exact'`
overrides the library's `@size:log2`.

```sh
$ export LD_PRELOAD=../tapir-pix/target/release/libtapir_pix.so
$ rm -r tapir-out      # start clean, so the globs below match one file

# Coarser: two significant bits for sizes, ignore paths, only resize and load
$ TAPIR_KEY='@size:sig/2; @path:drop' TAPIR_PROFILERS='bbv(fn=pix_resize;pix_load)' ./app
$ cat tapir-out/bbv.blocks.*.csv
block,hash,lib,function,label,args,calls,weight
1,3004698236796388856,pix,pix_load,pix_load(),,6,6
2,12563404756611859819,pix,pix_resize,"pix_resize(width=~128,height=~128,filter=1)",width@size=~128 height@size=~128 filter@mode=1,3,3
3,6405216706320849081,pix,pix_resize,"pix_resize(width=~1024,height=~1024,filter=1)",width@size=~1024 height@size=~1024 filter@mode=1,3,3

# Only the function name matters
$ TAPIR_PROFILERS='bbv(key=name)' ./app

# Every exact signature, ignoring the library's defaults
$ TAPIR_PROFILERS='bbv(key=exact)' ./app

# summary uses exact signatures unless told otherwise; key=default uses the library's rules
$ TAPIR_PROFILERS='summary(key=default)' ./app

# Two bbv instances with different keys in one run (bbv.* and bbv_fine.* files)
$ TAPIR_PROFILERS='bbv, bbv(as=bbv_fine, key=@size:exact)' ./app
```

> **Floats in keys.** An exact float is stored by its bit pattern, so
> `summary` (exact by default) prints
> `pix_blur(radius=4615063718147915776)`. For float arguments, use `drop` or
> `zeroone` (0, 1, -1 or other). The other buckets group floats by binary
> exponent, but their labels are hard to read.

## Choosing tags

Good tags describe a *role* that many functions share, not one parameter:

- Use the same tag for every parameter that should be bucketed the same
  way (`width` and `height` are both `size`).
- Separate tags that need different treatment (`size` is bucketed, `mode`
  is exact, `radius` is dropped).
- Untagged is fine for arguments that should always be exact.
- If you wrap a family of libraries, share one set of tags across them.
  That set is a [vocabulary](../guides/vocabularies.md). The `linalg`
  vocabulary, for example, uses `dim`, `count`, `stride`, `flag` and `scalar`.

The full rule syntax, every bucket type and the precedence rules are in
[Block keys and bucketing](../guides/keys.md).
