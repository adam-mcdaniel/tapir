//! The extension point: implement [`Profiler`] to add a new analysis.

use std::sync::{Mutex, MutexGuard};

use crate::event::Event;
use crate::output::Sink;

/// Context handed to [`Profiler::on_event`].
pub struct Ctx<'a> {
    pub(crate) sink: &'a Mutex<Sink>,
    /// Index of the interval currently open (0 before the first boundary).
    pub interval: u64,
}

impl Ctx<'_> {
    /// Output sink, for profilers that stream rows as they go. Locking it is
    /// cheap but not free; batch writes.
    pub fn sink(&self) -> MutexGuard<'_, Sink> {
        self.sink.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A modular analysis fed by tapir events.
///
/// The runtime wraps each profiler in its own mutex, so methods take
/// `&mut self` and implementations need no synchronization of their own.
///
/// Lifecycle:
/// - [`on_event`](Profiler::on_event) for every event that passes the
///   runtime's filters;
/// - [`interval`](Profiler::interval) whenever an interval is closed, either
///   *online* by an external driver calling `tapir_interval()`, or by the
///   profiler's own policy;
/// - [`finish`](Profiler::finish) once at process exit (or `tapir_flush()`).
pub trait Profiler: Send + 'static {
    /// Short name, used for option lookup, output stream names and
    /// `tapir_snapshot("<name>", ...)`.
    fn name(&self) -> &str;

    /// Return false if this profiler never reads call timestamps. When no
    /// active profiler needs them, the runtime skips the clock reads entirely
    /// (`t_start == t_end == 0`), which is most of the per-call overhead.
    fn needs_time(&self) -> bool {
        true
    }

    /// Return true to also receive [`Event::Enter`] before each call.
    fn wants_enter(&self) -> bool {
        false
    }

    fn on_event(&mut self, ev: &Event, cx: &Ctx);

    /// Close the current interval `index` and emit whatever belongs to it.
    fn interval(&mut self, _index: u64, _out: &mut Sink) {}

    /// Render the *current* interval as text without closing it. Used by
    /// `tapir_snapshot()` so an online consumer can embed tapir's data in its
    /// own output. Return false if this profiler has no snapshot form.
    fn snapshot(&mut self, _out: &mut String) -> bool {
        false
    }

    /// Like [`snapshot`](Profiler::snapshot), then close the interval
    /// *without* writing it to the sink (the caller owns the data now).
    fn take(&mut self, index: u64, out: &mut String) -> bool {
        let ok = self.snapshot(out);
        if ok {
            self.discard_interval(index);
        }
        ok
    }

    /// The current interval as a sparse vector of `(feature id, weight)`,
    /// for consumers that want numbers rather than text (`tapir_vector()`).
    /// With `take`, also close the interval without writing it. Return false
    /// if this profiler has no vector form.
    fn vector(&mut self, _take: bool, _out: &mut Vec<(u64, f64)>) -> bool {
        false
    }

    /// Reset per-interval state without emitting it (used by `take`).
    fn discard_interval(&mut self, _index: u64) {}

    /// Emit end-of-run output.
    fn finish(&mut self, out: &mut Sink);
}

/// A profiler from a closure, for one-off analyses:
///
/// ```ignore
/// b.add(tapir::profiler::from_fn("big_writes", |ev| {
///     if let tapir::Event::Call(c) = ev {
///         if c.tagged("bytes").filter_map(|v| v.as_i64()).any(|n| n > 1 << 20) { eprintln!("{c:?}") }
///     }
/// }));
/// ```
pub fn from_fn<F>(name: &'static str, f: F) -> impl Profiler
where
    F: FnMut(&Event) + Send + 'static,
{
    struct FnProfiler<F>(&'static str, F);
    impl<F: FnMut(&Event) + Send + 'static> Profiler for FnProfiler<F> {
        fn name(&self) -> &str {
            self.0
        }
        fn on_event(&mut self, ev: &Event, _: &Ctx) {
            (self.1)(ev)
        }
        fn needs_time(&self) -> bool {
            true
        }
        fn finish(&mut self, _: &mut Sink) {}
    }
    FnProfiler(name, f)
}
