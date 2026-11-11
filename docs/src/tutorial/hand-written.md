# A wrapper by hand

A tapir wrapper library is a Rust `cdylib` with two macro calls in it:

- `tapir::preload!` starts tapir's runtime when the library is loaded.
- `tapir::interpose!` declares the C functions to wrap and how to read each
  argument.

You will start with one function and then wrap the rest.

## Step 1: create the crate

```sh
cd work
cargo new --lib tapir-pix
```

Edit `tapir-pix/Cargo.toml`:

```toml
[package]
name = "tapir-pix"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]      # build a C-compatible shared library

[dependencies]
tapir = { path = "../tapir/crates/tapir" }
```

`crate-type = ["cdylib"]` makes Cargo produce `libtapir_pix.so`. The
hyphen in the package name becomes an underscore in the file name.

## Step 2: wrap one function

Replace `tapir-pix/src/lib.rs` with:

```rust
use std::ffi::c_void;

tapir::preload!(|b| {
    b.from_env("summary");
});

tapir::interpose! {
    lib "pix";
    fn pix_resize(img: *mut c_void => skip, width: i32 => val, height: i32 => val,
                  filter: i32 => val) -> i32;
}
```

Build it, then run the application with the wrapper preloaded:

```sh
$ cd tapir-pix && cargo build --release && cd ../pix
$ TAPIR_VERBOSE=1 LD_PRELOAD=../tapir-pix/target/release/libtapir_pix.so ./app
[tapir] pid 88 profilers: [summary]
done
[tapir] wrote tapir-out/summary.p88.csv
$ cat tapir-out/summary.p88.csv
lib,function,signature,calls,nested_calls,total_s,mean_s,min_s,max_s
pix,pix_resize,"pix_resize(width=1000,height=1000,filter=1)",1,0,0.000615294,...
pix,pix_resize,"pix_resize(width=1000,height=1002,filter=1)",1,0,0.000489876,...
pix,pix_resize,"pix_resize(width=1000,height=1001,filter=1)",1,0,0.000471376,...
pix,pix_resize,"pix_resize(width=128,height=129,filter=1)",1,0,0.000011959,...
pix,pix_resize,"pix_resize(width=128,height=128,filter=1)",1,0,0.000008667,...
pix,pix_resize,"pix_resize(width=128,height=130,filter=1)",1,0,0.000003709,...
```

The application was not rebuilt or changed, and every `pix_resize` call
was recorded with its arguments.

### What just happened

**`preload!`** puts a function in the ELF `.init_array` section, so the
dynamic loader runs it when `libtapir_pix.so` is loaded, before `main`.
That function calls `tapir::init` with your closure. The closure receives a
`Builder` and configures the runtime. Here, `b.from_env("summary")` means
"run the profilers listed in `TAPIR_PROFILERS`, or `summary` if it is
unset". tapir also registers an `atexit` handler that writes each
profiler's output when the program exits.

**`interpose!`** turns each `fn` line into an exported
`extern "C" fn pix_resize(...)`. Because the wrapper library is listed in
`LD_PRELOAD`, the dynamic linker binds the application's call to it rather
than to `libpix.so`. Roughly, each generated wrapper does this:

```rust
#[no_mangle]
pub unsafe extern "C" fn pix_resize(img: *mut c_void, width: i32, height: i32, filter: i32) -> i32 {
    static SPEC: FnSpec = FnSpec::new("pix", "pix_resize", &[
        ArgSpec::new("img", ""), ArgSpec::new("width", ""), ArgSpec::new("height", ""), ArgSpec::new("filter", ""),
    ]);
    // The next definition of the symbol after this library: the real one.
    // Looked up once with dlsym(RTLD_NEXT, "pix_resize"), then cached.
    let real: unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> i32 = /* ... */;
    let args = [skip(&img), val(&width), val(&height), val(&filter)];
    tapir::call(&SPEC, &args, move || real(img, width, height, filter))
}
```

`tapir::call` reads the clock, runs the real function, reads the clock
again, and hands a `Call` event (spec, captured values, times, nesting
depth, thread) to every active profiler. If recording is paused, or the
call comes from inside tapir itself, it calls the real function and
nothing else.

**`lib "pix";`** sets a *library tag* for the functions that follow it.
The tag shows up in output and can be used to filter (`summary(lib=pix)`).
It does not have to match a file name.

### The argument syntax

Each argument is written `name: Type => mode`, or `name: Type => mode "tag"`:

- **`name`** is used in labels and by bucketing rules. Use the C parameter
  name.
- **`Type`** must have the same ABI as the C parameter. See the table
  below.
- **`mode`** says *how* to read the value. `val` reads the argument itself,
  and `skip` reads nothing. There are seven modes, covered in the next step.
- **`"tag"`** (optional) says what the value *means*. That comes in the
  [next chapter](tags-and-keys.md).

The return type comes after `->`. Leave it out for `void` functions.

> **Get the types right.** The wrapper forwards its arguments to the real
> function unchanged, so a wrong type corrupts the call itself, not just
> the profile. Declaring `i32` for a `size_t`, for example, truncates
> large values and can shift the following arguments.

| C type | Rust type in `interpose!` |
|---|---|
| `int`, `enum`, `int32_t` | `i32` |
| `unsigned`, `uint32_t` | `u32` |
| `long`, `long long`, `int64_t` (LP64) | `i64` |
| `size_t` / `ssize_t` | `usize` / `isize` |
| `float` / `double` | `f32` / `f64` |
| `char` | `c_char` |
| any pointer, handle, function pointer | `*const c_void` or `*mut c_void` |
| `const char *` read as a string | `*const c_char` |
| `const T *` read through (`deref`) | `*const T` |

## Step 3: wrap the whole API

Wrap the rest of `pix`. Each parameter kind needs a different capture mode:

```rust
use std::ffi::{c_char, c_void};

tapir::preload!(|b| {
    b.from_env("summary");
});

tapir::interpose! {
    lib "pix";
    fn pix_load(path: *const c_char => cstr) -> *mut c_void;
    fn pix_save(img: *mut c_void => skip, path: *const c_char => cstr) -> i32;
    fn pix_resize(img: *mut c_void => skip, width: i32 => val, height: i32 => val,
                  filter: i32 => val) -> i32;
    fn pix_blur(img: *mut c_void => skip, radius: f32 => val) -> i32;
    fn pix_convert(img: *mut c_void => skip, format: c_char => chr) -> i32;
    fn pix_map(img: *mut c_void => skip, f: *const c_void => sym) -> i32;
    fn pix_free(img: *mut c_void => skip);
}
```

| Parameter | Mode | Why |
|---|---|---|
| `img` (handle) | `skip` | the address changes every run and says nothing about the work |
| `path` | `cstr` | read the NUL-terminated string |
| `width`, `height`, `filter` | `val` | plain integers (a C enum is an `int`) |
| `radius` | `val` | floats are captured as numbers too |
| `format` | `chr` | record `'g'` as a character, not as 103 |
| `f` (callback) | `sym` | report the code address by symbol name |

`pix_bytes` is left out on purpose: a call that is not declared is not
intercepted and runs as usual.

The compiler checks the modes. `cstr` on an `i32`, or `chr` on a pointer,
fails to build, because each mode is a trait that only fitting types
implement.

Rebuild and look at a trace:

```sh
$ TAPIR_PROFILERS=trace LD_PRELOAD=../tapir-pix/target/release/libtapir_pix.so ./app
$ head -8 tapir-out/trace.*.csv
t_ns,duration_ns,thread,depth,interval,kind,lib,function,args
83458841196,3625,0,0,0,call,pix,pix_load,"path=""cat.png"""
83458847071,14959,0,0,0,call,pix,pix_resize,width=128 height=128 filter=1
83458863905,41,0,0,0,call,pix,pix_blur,radius=1.5e0
83458865363,83,0,0,0,call,pix,pix_convert,format=g
83458866030,17666,0,0,0,call,pix,pix_map,f=app+0xbe0
83458886530,4958,0,0,0,call,pix,pix_save,"path=""out.png"""
83458891988,292,0,0,0,call,pix,pix_free,
```

A few things to notice:

- Skipped arguments are left out of `args` entirely.
- The `static` callback `clamp` has no symbol, so it is reported as
  `app+0xbe0`, its offset inside the `app` executable. That stays the same
  from run to run even with address-space randomization. The exported
  `invert` appears as `f=invert` further down.
- `depth` is 0 for calls made by the application. A wrapped library that
  calls *another* wrapped library produces calls with `depth > 0`.

## Step 4: let users choose profilers

`b.from_env("summary")` already lets the user pick profilers with
`TAPIR_PROFILERS`. Choose the default your users need most. Most wrapper
libraries use `"bbv,summary"`:

```rust
tapir::preload!(|b| {
    b.from_env("bbv,summary");
});
```

The `Builder` has a few more methods you may want in `preload!`:

| Method | Effect |
|---|---|
| `b.from_env(default)` | enable `TAPIR_PROFILERS`, or `default` |
| `b.default_key(rules)` | the library's bucketing rules (next chapter) |
| `b.factory(name, f)` | make a custom profiler selectable by name ([Writing a profiler](custom-profiler.md)) |
| `b.add(profiler)` | always run this profiler instance |
| `b.enable(Opts::new("bbv").with("weight", "time"))` | enable a named profiler from code |
| `b.output(template)` | output path template (overrides `TAPIR_OUT`) |
| `b.after_call(hook)` | run `hook()` after each call, inside the timed region |
| `b.paused()` | start paused until `tapir_enable(1)` |

Next you will make the profiles more useful by saying what the arguments
mean.
