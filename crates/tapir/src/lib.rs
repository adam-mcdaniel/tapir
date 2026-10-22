//! # tapir — Tracing And Profiling Interposition Runtime
//!
//! General-purpose infrastructure for wrapping C functions, capturing their
//! arguments, and feeding the calls to modular profilers that can be sampled
//! online or dumped at exit. tapir has no knowledge of any particular library:
//! what an argument means is a free-form *tag* chosen by whoever writes the
//! wrapper.
//!
//! The pieces:
//! - [`interpose!`] declares wrappers (`name: Type => mode "tag"`), or
//!   `tapir-gen` generates them from C headers. [`capture`] lists the modes.
//! - [`call`] is the hot path every wrapper goes through; it can also
//!   instrument ordinary code by hand.
//! - [`Profiler`] is the extension point. Built-ins: [`profilers::Bbv`]
//!   (basic-block vectors over bucketed calls), [`profilers::Summary`],
//!   [`profilers::Trace`].
//! - [`KeyPolicy`] decides which calls are "the same block", by rules over
//!   tags and argument names (`@size:log2; mode:exact`).
//! - [`output`] renders tables as CSV/TSV/JSONL (or raw text) into per-rank
//!   files that are never overwritten.
//! - The C ABI (`include/tapir.h`) lets other code drive intervals
//!   (`tapir_interval`), pull data online (`tapir_vector`, `tapir_snapshot`),
//!   and report calls tapir does not wrap (`tapir_record`).
//!
//! A complete preload library:
//!
//! ```ignore
//! use std::ffi::{c_char, c_void};
//! tapir::preload!(|b| {
//!     b.default_key("@bytes:log2");
//!     b.from_env("summary");
//! });
//! tapir::interpose! {
//!     lib "libc";
//!     fn fopen(path: *const c_char => cstr "path", mode: *const c_char => cstr) -> *mut c_void;
//!     fn fwrite(p: *const c_void => skip, size: usize => val "bytes", n: usize => val "count",
//!               f: *mut c_void => skip) -> usize;
//! }
//! ```

pub mod capi;
pub mod capture;
pub mod config;
pub mod event;
pub mod interpose;
pub mod key;
pub mod output;
pub mod profiler;
pub mod profilers;
mod runtime;
pub mod sys;
pub mod value;

pub use config::{Filter, Opts};
pub use event::{Call, Event};
pub use key::{BlockTable, Bucket, KeyPolicy, Kind};
pub use output::{Cell, Format, Sink, Table};
pub use profiler::{from_fn, Ctx, Profiler};
pub use runtime::{call, get as runtime, init, Builder, Factory, Runtime};
pub use value::{ArgSpec, FnSpec, Value};

/// Make the enclosing `cdylib` a self-starting tapir library: initializes the
/// runtime from an ELF constructor when the library is loaded (via
/// `LD_PRELOAD` or normal linking), running `setup` to choose profilers.
///
/// ```ignore
/// tapir::preload!(|b| {
///     b.default_key("@size:log2");       // this library's bucketing defaults
///     b.from_env("bbv");                 // honour TAPIR_PROFILERS, default "bbv"
///     b.add(MyProfiler::default());      // plus a custom one
/// });
/// ```
#[macro_export]
macro_rules! preload {
    ($setup:expr) => {
        #[used]
        #[link_section = ".init_array"]
        static __TAPIR_PRELOAD_INIT: extern "C" fn() = {
            extern "C" fn __tapir_preload_init() {
                $crate::init($setup);
            }
            __tapir_preload_init
        };
    };
}
