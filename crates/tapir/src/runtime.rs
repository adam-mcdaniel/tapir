//! Process-global runtime: the profiler set, the hot call path, intervals,
//! and end-of-run flushing.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::config::{self, Opts};
use crate::event::{Call, Event};
use crate::output::{Format, Identity, Sink};
use crate::profiler::{Ctx, Profiler};
use crate::profilers;
use crate::sys;
use crate::value::{FnSpec, Value};

/// Builds a profiler from its options; registered by name so it can be
/// selected from `TAPIR_PROFILERS`.
pub type Factory = Box<dyn Fn(&Opts) -> Result<Box<dyn Profiler>, String>>;

struct Slot {
    name: String,
    wants_enter: bool,
    p: Mutex<Box<dyn Profiler>>,
}

pub struct Runtime {
    slots: Vec<Slot>,
    any_enter: bool,
    timed: bool,
    sink: Mutex<Sink>,
    enabled: AtomicBool,
    finished: AtomicBool,
    /// Set by the first event or interval. Processes that never see one (shells
    /// and helpers that inherit `LD_PRELOAD`) write nothing at exit.
    active: AtomicBool,
    next_interval: AtomicU64,
    verbose: bool,
    post_call: Option<fn()>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

enum Pending {
    Ready(Box<dyn Profiler>),
    Named(Opts),
}

/// Configures the runtime before it starts. Obtained via [`init`].
pub struct Builder {
    /// Profiler instances and named profilers to instantiate, in order.
    /// Named ones are built in `init`, after setup, so they see the final
    /// default key and format whatever order setup calls things in.
    pending: Vec<Pending>,
    factories: Vec<(&'static str, Factory)>,
    out: Option<String>,
    format: Option<Format>,
    key: String,
    enabled: bool,
    verbose: bool,
    post_call: Option<fn()>,
    errors: Vec<String>,
}

impl Builder {
    fn new() -> Self {
        let env = |k| std::env::var(k).ok().filter(|v: &String| !v.is_empty());
        let mut b = Builder {
            pending: Vec::new(),
            factories: Vec::new(),
            out: env("TAPIR_OUT"),
            format: None,
            key: String::new(),
            enabled: env("TAPIR_START").is_none_or(|v| v != "paused" && v != "0"),
            verbose: env("TAPIR_VERBOSE").is_some_and(|v| v != "0"),
            post_call: None,
            errors: Vec::new(),
        };
        if let Some(f) = env("TAPIR_FORMAT") {
            match Format::parse(&f) {
                Some(f) => b.format = Some(f),
                None => b.errors.push(format!("TAPIR_FORMAT: unknown format `{f}`")),
            }
        }
        profilers::register_builtins(&mut b);
        b
    }

    /// Add a profiler instance directly.
    pub fn add(&mut self, p: impl Profiler) -> &mut Self {
        self.pending.push(Pending::Ready(Box::new(p)));
        self
    }

    /// Default key rules for this library (see [`KeyPolicy`](crate::KeyPolicy)),
    /// e.g. `"@size:log2;@scale:drop"`. Profilers that bucket calls start from
    /// these; `TAPIR_KEY` is applied on top, then each profiler's `key=`.
    /// Repeated calls append.
    pub fn default_key(&mut self, rules: &str) -> &mut Self {
        if !self.key.is_empty() {
            self.key.push(';');
        }
        self.key.push_str(rules);
        self
    }

    /// Make a profiler selectable by name from `TAPIR_PROFILERS`:
    /// `b.factory("flops", Flops::from_opts)` or a closure returning
    /// `Result<P, String>` for any `P: Profiler`.
    pub fn factory<P, F>(&mut self, name: &'static str, f: F) -> &mut Self
    where
        P: Profiler,
        F: Fn(&Opts) -> Result<P, String> + 'static,
    {
        self.factories.retain(|(n, _)| *n != name);
        self.factories.push((name, Box::new(move |o: &Opts| f(o).map(|p| Box::new(p) as Box<dyn Profiler>))));
        self
    }

    /// Enable a registered profiler by name with options. It is
    /// instantiated when setup finishes.
    pub fn enable(&mut self, opts: Opts) -> &mut Self {
        self.pending.push(Pending::Named(opts));
        self
    }

    fn build(mut self) -> (Vec<Box<dyn Profiler>>, Builder) {
        let mut key = std::mem::take(&mut self.key);
        if let Some(env) = std::env::var("TAPIR_KEY").ok().filter(|v| !v.trim().is_empty()) {
            if !key.is_empty() {
                key.push(';');
            }
            key.push_str(&env);
        }
        if let Err(e) = crate::KeyPolicy::parse(&key, crate::KeyPolicy::exact()) {
            self.errors.push(format!("default key: {e}"));
        }
        let mut out = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            let mut opts = match p {
                Pending::Ready(p) => {
                    out.push(p);
                    continue;
                }
                Pending::Named(o) => o,
            };
            opts.default_format = opts.default_format.or(self.format);
            opts.default_key = key.clone();
            let kind = opts.name.clone();
            // `bbv(as=bbv2, ...)` runs a second, separately named instance.
            if let Some(alias) = opts.get("as").map(str::to_owned) {
                opts.name = alias;
            }
            match self.factories.iter().find(|(n, _)| *n == kind) {
                Some((_, f)) => match f(&opts) {
                    Ok(p) => out.push(p),
                    Err(e) => self.errors.push(e),
                },
                None => {
                    let known: Vec<_> = self.factories.iter().map(|(n, _)| *n).collect();
                    self.errors.push(format!("unknown profiler `{kind}` (known: {})", known.join(", ")));
                }
            }
        }
        (out, self)
    }

    /// Enable the profilers listed in `TAPIR_PROFILERS`, or `default` if it
    /// is unset (e.g. `"summary"`).
    pub fn from_env(&mut self, default: &str) -> &mut Self {
        let spec = std::env::var("TAPIR_PROFILERS").unwrap_or_else(|_| default.to_owned());
        match config::parse_list(&spec) {
            Ok(list) => {
                for o in list {
                    self.enable(o);
                }
            }
            Err(e) => self.errors.push(format!("TAPIR_PROFILERS: {e}")),
        }
        self
    }

    /// Output path template (overrides `TAPIR_OUT`).
    pub fn output(&mut self, template: &str) -> &mut Self {
        self.out = Some(template.to_owned());
        self
    }

    /// Run `hook` after every recorded call, inside the timed region (e.g. to
    /// wait for asynchronous work so it is timed to completion).
    pub fn after_call(&mut self, hook: fn()) -> &mut Self {
        self.post_call = Some(hook);
        self
    }

    /// Start with recording paused until `tapir_enable(1)`.
    pub fn paused(&mut self) -> &mut Self {
        self.enabled = false;
        self
    }
}

/// Start the runtime. Call once, early (the [`preload!`](crate::preload)
/// macro calls it from an ELF constructor). Later calls are ignored.
/// Returns false if the runtime was already initialized.
pub fn init(setup: impl FnOnce(&mut Builder)) -> bool {
    let mut fresh = false;
    RUNTIME.get_or_init(|| {
        fresh = true;
        let mut b = Builder::new();
        setup(&mut b);
        let (profilers, b) = b.build();
        for e in &b.errors {
            eprintln!("[tapir] error: {e}");
        }
        let timed = profilers.iter().any(|p| p.needs_time());
        let slots: Vec<Slot> = profilers
            .into_iter()
            .map(|p| Slot { name: p.name().to_owned(), wants_enter: p.wants_enter(), p: Mutex::new(p) })
            .collect();
        if b.verbose {
            let names: Vec<_> = slots.iter().map(|s| s.name.as_str()).collect();
            eprintln!("[tapir] pid {} profilers: [{}]", std::process::id(), names.join(", "));
        }
        Runtime {
            any_enter: slots.iter().any(|s| s.wants_enter),
            timed: timed || b.post_call.is_some(),
            slots,
            sink: Mutex::new(Sink::new(b.out, Identity::detect())),
            enabled: AtomicBool::new(b.enabled),
            finished: AtomicBool::new(false),
            active: AtomicBool::new(false),
            next_interval: AtomicU64::new(0),
            verbose: b.verbose,
            post_call: b.post_call,
        }
    });
    if fresh {
        extern "C" fn on_exit() {
            if let Some(rt) = RUNTIME.get() {
                rt.finish();
            }
        }
        unsafe { sys::atexit(on_exit) };
    }
    fresh
}

pub fn get() -> Option<&'static Runtime> {
    RUNTIME.get()
}

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Set while tapir itself is running, so wrapped calls made by profilers
    /// (or by the runtime) pass straight through instead of recursing.
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static THREAD: Cell<u32> = const { Cell::new(u32::MAX) };
}

static NEXT_THREAD: AtomicU32 = AtomicU32::new(0);

pub(crate) fn thread_index() -> u32 {
    THREAD.with(|t| {
        let v = t.get();
        if v != u32::MAX {
            return v;
        }
        let v = NEXT_THREAD.fetch_add(1, Relaxed);
        t.set(v);
        v
    })
}

/// Run `real` as a recorded call of `spec`. This is what every generated
/// wrapper calls; it is also usable by hand for code that is not interposed
/// (e.g. instrumenting your own functions).
#[inline]
pub fn call<R>(spec: &'static FnSpec, args: &[Value], real: impl FnOnce() -> R) -> R {
    let rt = match RUNTIME.get() {
        Some(rt) if rt.enabled.load(Relaxed) && !BUSY.with(Cell::get) => rt,
        _ => return real(),
    };
    let depth = DEPTH.with(|d| d.replace(d.get() + 1));
    let clock = || if rt.timed { sys::now_ns() } else { 0 };
    let t_start = clock();
    let mut c = Call { spec, args, t_start, t_end: t_start, depth, thread: thread_index() };
    if rt.any_enter {
        rt.dispatch(&Event::Enter(&c), true);
    }
    let r = real();
    if let Some(hook) = rt.post_call {
        let _busy = Busy::enter();
        hook();
    }
    c.t_end = clock();
    DEPTH.with(|d| d.set(depth));
    rt.dispatch(&Event::Call(&c), false);
    r
}

/// Guard that marks the current thread as inside tapir.
struct Busy(bool);
impl Busy {
    fn enter() -> Busy {
        Busy(BUSY.with(|b| b.replace(true)))
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with(|b| b.set(self.0));
    }
}

fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panicking profiler must not take the application down with it.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Runtime {
    /// Deliver an event to every profiler (only `wants_enter` ones for `Enter`).
    pub fn dispatch(&self, ev: &Event, enter: bool) {
        if self.finished.load(Relaxed) {
            return;
        }
        if !self.active.load(Relaxed) {
            self.active.store(true, Relaxed);
        }
        let _busy = Busy::enter();
        let cx = Ctx { sink: &self.sink, interval: self.next_interval.load(Relaxed) };
        for s in &self.slots {
            if !enter || s.wants_enter {
                lock(&s.p).on_event(ev, &cx);
            }
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Relaxed)
    }

    /// Close the current interval in every profiler. `index` defaults to a
    /// running counter. Returns the index that was closed.
    pub fn close_interval(&self, index: Option<u64>) -> u64 {
        let idx = match index {
            Some(i) => {
                self.next_interval.store(i + 1, Relaxed);
                i
            }
            None => self.next_interval.fetch_add(1, Relaxed),
        };
        if self.finished.load(Relaxed) {
            return idx;
        }
        self.active.store(true, Relaxed);
        let _busy = Busy::enter();
        // Lock order is always profiler -> sink (as in `dispatch`).
        for s in &self.slots {
            let mut p = lock(&s.p);
            p.interval(idx, &mut lock(&self.sink));
        }
        lock(&self.sink).flush();
        idx
    }

    /// Run `f` on the profiler called `name` (first match).
    pub fn with_profiler<T>(&self, name: &str, f: impl FnOnce(&mut dyn Profiler) -> T) -> Option<T> {
        let _busy = Busy::enter();
        let s = self.slots.iter().find(|s| s.name == name)?;
        let mut g = lock(&s.p);
        Some(f(&mut **g))
    }

    /// Text of the current interval of profiler `name` (see
    /// [`Profiler::snapshot`]); with `take`, also close it without writing.
    pub fn snapshot(&self, name: &str, take: bool) -> Option<String> {
        let idx = if take { self.next_interval.fetch_add(1, Relaxed) } else { 0 };
        self.with_profiler(name, |p| {
            let mut s = String::new();
            let ok = if take { p.take(idx, &mut s) } else { p.snapshot(&mut s) };
            ok.then_some(s)
        })
        .flatten()
    }

    /// Sparse vector of the current interval of profiler `name` (see
    /// [`Profiler::vector`]). With `take`, the interval is closed only if the
    /// vector has at most `cap` entries; otherwise `Err(len)` and nothing
    /// changes. Check and take happen under one lock. `None` if there is no
    /// such profiler or it has no vector form.
    pub fn vector(&self, name: &str, take: bool, cap: usize) -> Option<Result<Vec<(u64, f64)>, usize>> {
        self.with_profiler(name, |p| {
            let mut v = Vec::new();
            if !p.vector(false, &mut v) {
                return None;
            }
            if !take {
                return Some(Ok(v));
            }
            if v.len() > cap {
                return Some(Err(v.len()));
            }
            v.clear();
            p.vector(true, &mut v);
            self.next_interval.fetch_add(1, Relaxed);
            Some(Ok(v))
        })
        .flatten()
    }

    /// Emit end-of-run output. Idempotent; called automatically at exit.
    pub fn finish(&self) {
        if self.finished.swap(true, AcqRel) {
            return;
        }
        if !self.active.load(Relaxed) {
            if self.verbose {
                eprintln!("[tapir] pid {}: no events, nothing written", std::process::id());
            }
            return;
        }
        let _busy = Busy::enter();
        for s in &self.slots {
            let mut p = lock(&s.p);
            p.finish(&mut lock(&self.sink));
        }
        let mut sink = lock(&self.sink);
        sink.flush();
        if self.verbose {
            for p in &sink.created {
                eprintln!("[tapir] wrote {}", p.display());
            }
        }
    }
}
