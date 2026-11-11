# Writing a profiler

The built-in profilers (`bbv`, `summary`, `trace`) are general. Sometimes
you want a number specific to your library. For `pix`, say you want to know
how many pixels each phase resized to, counting only large images. In this
chapter you will write a `pixels` profiler and make it selectable by name.

## The `Profiler` trait

```rust
pub trait Profiler: Send + 'static {
    fn name(&self) -> &str;                                   // stream name, lookup name
    fn needs_time(&self) -> bool { true }                     // false: skip clock reads
    fn wants_enter(&self) -> bool { false }                   // true: also get Event::Enter
    fn on_event(&mut self, ev: &Event, cx: &Ctx);             // every call / marker
    fn interval(&mut self, index: u64, out: &mut Sink) {}     // an interval was closed
    fn snapshot(&mut self, out: &mut String) -> bool { false } // text of the open interval
    fn vector(&mut self, take: bool, out: &mut Vec<(u64, f64)>) -> bool { false }
    fn discard_interval(&mut self, index: u64) {}
    fn take(&mut self, index: u64, out: &mut String) -> bool { /* snapshot + discard */ }
    fn finish(&mut self, out: &mut Sink);                     // end of run
}
```

The runtime puts each profiler behind its own mutex, so the methods take
`&mut self` and need no locking of their own. A profiler that calls a
wrapped function (writing a file with `fwrite` under `libtapir_posixio`,
for example) is not recorded: calls made from inside tapir go straight to
the real function.

The lifecycle:

1. `on_event` for every event: a completed `Event::Call`, an
   `Event::Marker` from the application, and `Event::Enter` if you asked
   for it.
2. `interval` whenever the application closes an interval
   (`tapir_interval`). A profiler with its own interval policy, such as
   `bbv(every=calls:N)`, closes its own intervals internally and does not
   affect the others.
3. `finish` once, at exit or on `tapir_flush()`.

## Step 1: the profiler

Create `tapir-pix/src/pixels.rs`:

```rust
//! `pixels`: how many pixels each interval resized to.
//!
//! TAPIR_PROFILERS="pixels(min=100000)" counts only resizes whose output
//! has at least 100000 pixels.

use tapir::{Cell, Ctx, Event, Format, Opts, Profiler, Sink, Table};

pub struct Pixels {
    name: String,
    min: i64,
    format: Format,
    cur: (u64, i64),  // resizes and pixels in the open interval
    rows: Table,      // one row per closed interval
}

impl Pixels {
    pub fn from_opts(o: &Opts) -> Result<Pixels, String> {
        Ok(Pixels {
            name: o.name.clone(),
            min: o.parse_or("min", 0)?,
            format: o.format(Format::Csv)?,
            cur: (0, 0),
            rows: Table::new(&["interval", "resizes", "megapixels"]),
        })
    }
}

impl Profiler for Pixels {
    fn name(&self) -> &str {
        &self.name
    }

    fn needs_time(&self) -> bool {
        false // we never look at t_start/t_end
    }

    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        let Event::Call(c) = ev else { return };
        if c.depth > 0 || c.spec.name != "pix_resize" {
            return;
        }
        let px: i64 = c.tagged("size").filter_map(|v| v.as_i64()).product();
        if px >= self.min {
            self.cur.0 += 1;
            self.cur.1 += px;
        }
    }

    fn interval(&mut self, index: u64, _: &mut Sink) {
        let (n, px) = std::mem::take(&mut self.cur);
        self.rows.push(vec![index.into(), n.into(), Cell::Float(px as f64 * 1e-6)]);
    }

    fn snapshot(&mut self, out: &mut String) -> bool {
        out.push_str(&format!("{} resizes, {} pixels\n", self.cur.0, self.cur.1));
        true
    }

    fn finish(&mut self, out: &mut Sink) {
        if self.cur.0 > 0 || self.rows.rows.is_empty() {
            let i = self.rows.rows.len() as u64;
            self.interval(i, out);
        }
        if let Err(e) = out.table(&self.name, self.format, &self.rows) {
            eprintln!("[tapir] {}: {e}", self.name);
        }
    }
}
```

Here is what each piece does.

**Reading arguments.** A `Call` gives you:

| Field or method | Meaning |
|---|---|
| `c.spec.lib`, `c.spec.name` | library tag and function name |
| `c.arg("width")` | the value of the parameter with that name |
| `c.tagged("size")` | values of every parameter with that tag, in order |
| `c.named_args()` | `(name, tag, value)` for every parameter |
| `c.t_start`, `c.t_end`, `c.duration_ns()` | monotonic nanoseconds (0 if no profiler needs time) |
| `c.depth` | 0 for calls made by the application, >0 for nested calls |
| `c.thread` | small per-process thread index |

Values are `tapir::Value`: `Int`, `UInt`, `Float`, `Char`, `Str`, `Sym`,
`Ptr`, or `None` for skipped arguments and null pointers. Use `as_i64()`,
`as_f64()` and `as_str()` to read them. A `Str` borrows the application's
memory and is only valid during `on_event`, so copy it if you need to keep
it.

Selecting by **tag**, not by parameter name, is what makes a profiler
reusable. `Pixels` would work unchanged for any function whose sizes are
tagged `size`.

**Options.** `Opts` holds the `key=value` pairs from `TAPIR_PROFILERS`.
`o.parse_or("min", 0)?` parses an option or uses the default, and
`o.format(Format::Csv)?` honours `format=` and `TAPIR_FORMAT`. Returning
`Err(message)` prints `[tapir] error: message` and leaves the profiler out,
without stopping the program. `o.name` is the instance name, which differs
from `"pixels"` when the user writes `pixels(as=big)`.

**Output.** A `Table` is a list of rows with fixed columns.
`out.table(stream, format, &table)` appends it to the stream's file, which
is named `pixels.p<pid>.csv` with the default template. The header is
written once. For a format of your own, `out.raw(stream, ext)` returns a
writer.

**`needs_time`.** Reading the clock twice per call is most of tapir's
per-call cost. If no active profiler needs time, tapir skips the clock
reads entirely.

## Step 2: register it

In `lib.rs`, add the module and register a factory under a name:

```rust
tapir::preload!(|b| {
    b.default_key("@size:log2; @radius:drop");
    b.factory("pixels", pixels::Pixels::from_opts);
    b.from_env("bbv,summary");
});

include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));

mod pixels;
```

`b.factory` registers a name. Nothing runs until that name appears in
`TAPIR_PROFILERS` (or in `from_env`'s default). Profilers are built after
the setup closure returns, so the order of the calls inside `preload!`
does not matter.

## Step 3: run it

`pixels` only produces one row per interval. The next chapter makes the
application close intervals. For now, the whole run is one interval:

```sh
$ TAPIR_PROFILERS='pixels(min=100000), summary' LD_PRELOAD=$LIB ./app
$ cat tapir-out/pixels.*.csv
interval,resizes,megapixels
0,3,3.0029999999999997
```

Only the three resizes to about 1000×1000 reach 100000 pixels. Floats are
written with full precision.

## Quick one-off profilers: `from_fn`

For a check you don't need to configure or write to a file, a closure is
enough:

```rust
tapir::preload!(|b| {
    b.add(tapir::from_fn("huge", |ev| {
        if let tapir::Event::Call(c) = ev {
            if c.tagged("size").filter_map(|v| v.as_i64()).any(|n| n > 4096) {
                eprintln!("huge {}: {:?}", c.spec.name, c.args);
            }
        }
    }));
    b.from_env("summary");
});
```

`b.add` always runs the profiler, whatever `TAPIR_PROFILERS` says.

## Going further: reuse tapir's bucketing

If your profiler groups calls the way `bbv` and `summary` do, use
`KeyPolicy` (the rules) and `BlockTable` (key to dense id). Then users'
`TAPIR_KEY` and the library's default key apply to your profiler too. This
one reports the slowest call per block:

```rust
use tapir::{BlockTable, Ctx, Event, Format, KeyPolicy, Opts, Profiler, Sink, Table};

struct Slowest {
    name: String,
    policy: KeyPolicy,
    blocks: BlockTable,
    worst: Vec<u64>,
    scratch: Vec<i64>,
}

impl Slowest {
    fn from_opts(o: &Opts) -> Result<Slowest, String> {
        // o.default_key = the library's default_key rules + TAPIR_KEY
        let mut policy = KeyPolicy::parse(&o.default_key, KeyPolicy::exact())?;
        if let Some(extra) = o.get("key") {
            policy = KeyPolicy::parse(extra, policy)?;
        }
        Ok(Slowest { name: o.name.clone(), policy, blocks: BlockTable::default(),
                     worst: Vec::new(), scratch: Vec::new() })
    }
}

impl Profiler for Slowest {
    fn name(&self) -> &str { &self.name }

    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        let Event::Call(c) = ev else { return };
        self.policy.key_into(c, &mut self.scratch);                     // bucketed key
        let id = self.blocks.intern(c.spec, &self.scratch) as usize;    // dense id
        if id == self.worst.len() {
            self.worst.push(0);
        }
        self.worst[id] = self.worst[id].max(c.duration_ns());
    }

    fn finish(&mut self, out: &mut Sink) {
        let mut t = Table::new(&["block", "max_ns"]);
        for id in 0..self.blocks.len() as u32 {
            let (spec, key) = self.blocks.entry(id);
            t.push(vec![self.policy.describe(spec, key).into(), self.worst[id as usize].into()]);
        }
        let _ = out.table(&self.name, Format::Csv, &t);
    }
}
```

`examples/fakeblas/src/lib.rs` has another complete profiler (GEMM FLOPs
per interval), and `crates/tapir/src/profilers/` has the built-ins, which
are written against the same public API.
