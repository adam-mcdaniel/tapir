//! Events delivered to profilers.

use crate::value::{FnSpec, Value};

/// One completed call: of a wrapped function, or reported by another tool
/// through `tapir_record` (anything with a name, arguments and a time span).
#[derive(Debug)]
pub struct Call<'a> {
    pub spec: &'static FnSpec,
    /// Captured arguments, parallel to `spec.args`.
    pub args: &'a [Value<'a>],
    /// Monotonic nanoseconds at entry and exit (0 if no profiler needs time).
    pub t_start: u64,
    pub t_end: u64,
    /// 0 for a call made by the application; >0 for a call made while
    /// another wrapped call was running on the same thread (a wrapped
    /// library calling another wrapped library).
    pub depth: u32,
    /// Small dense per-process thread index (0 = first thread seen).
    pub thread: u32,
}

impl<'a> Call<'a> {
    pub fn duration_ns(&self) -> u64 {
        self.t_end.saturating_sub(self.t_start)
    }

    /// Iterate `(arg name, tag, value)`.
    pub fn named_args(&self) -> impl Iterator<Item = (&'static str, &'static str, Value<'a>)> + '_ {
        self.spec.args.iter().zip(self.args).map(|(a, v)| (a.name, a.tag, *v))
    }

    /// Value of the argument called `name`.
    pub fn arg(&self, name: &str) -> Option<Value<'a>> {
        self.named_args().find(|(n, _, _)| *n == name).map(|(_, _, v)| v)
    }

    /// Values of the arguments tagged `tag`, in declaration order.
    pub fn tagged<'s>(&'s self, tag: &'s str) -> impl Iterator<Item = Value<'a>> + 's {
        self.named_args().filter(move |(_, t, _)| *t == tag).map(|(_, _, v)| v)
    }
}

#[derive(Debug)]
pub enum Event<'a> {
    /// A wrapped call is about to run. Only delivered to profilers that
    /// return `true` from [`Profiler::wants_enter`](crate::Profiler::wants_enter).
    Enter(&'a Call<'a>),
    /// A call returned (or was reported).
    Call(&'a Call<'a>),
    /// A named marker from `tapir_marker`, e.g. the start of a region of
    /// interest.
    Marker { name: &'a str, begin: bool, t: u64 },
}
