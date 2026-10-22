//! The C ABI, declared in `include/tapir.h`.
//!
//! Every tapir-based library exports these symbols. When several are loaded
//! (say one per wrapped library), the dynamic linker binds the
//! application's call to the first; each implementation handles its own
//! runtime and then forwards to the next definition (`dlsym(RTLD_NEXT)`), so
//! one call from the application reaches all of them.
//!
//! # Safety
//!
//! All string arguments must be NUL-terminated or null; array arguments must
//! hold at least `nargs` elements; `buf` must be writable for `cap` bytes.
#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_int, c_void, CStr};

use crate::event::{Call, Event};
use crate::runtime;
use crate::sys;
use crate::value::{FnSpec, Value};

macro_rules! forward {
    ($name:literal, $ty:ty, $($arg:expr),*) => {{
        let p = unsafe { sys::dlsym(sys::RTLD_NEXT, concat!($name, "\0").as_ptr() as *const c_char) };
        if !p.is_null() {
            let f: $ty = unsafe { std::mem::transmute::<*mut c_void, $ty>(p) };
            #[allow(unused_unsafe)]
            Some(unsafe { f($($arg),*) })
        } else {
            None
        }
    }};
}

unsafe fn cstr<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        CStr::from_ptr(p).to_str().unwrap_or("")
    }
}

/// 1 if a tapir runtime is initialized in this process.
#[no_mangle]
pub extern "C" fn tapir_active() -> c_int {
    runtime::get().is_some() as c_int
}

/// Pause (0) or resume (non-zero) recording.
#[no_mangle]
pub extern "C" fn tapir_enable(on: c_int) {
    if let Some(rt) = runtime::get() {
        rt.set_enabled(on != 0);
    }
    forward!("tapir_enable", extern "C" fn(c_int), on);
}

/// Close the current interval in every profiler. `index` < 0 means "next".
/// Returns the index closed (of the first runtime in the chain).
#[no_mangle]
pub extern "C" fn tapir_interval(index: i64) -> i64 {
    let idx = runtime::get().map(|rt| rt.close_interval((index >= 0).then_some(index as u64)) as i64);
    let next = forward!("tapir_interval", extern "C" fn(i64) -> i64, index);
    idx.or(next).unwrap_or(-1)
}

/// Copy the current interval of profiler `name` as text into `buf` (always
/// NUL-terminated when `cap > 0`). With `take != 0` the interval is also
/// closed without being written to tapir's own output, i.e. the caller now
/// owns that data. Returns the full length (excluding NUL), or -1 if no
/// loaded tapir library has a profiler called `name`.
#[no_mangle]
pub unsafe extern "C" fn tapir_snapshot(name: *const c_char, take: c_int, buf: *mut c_char, cap: usize) -> i64 {
    if let Some(text) = runtime::get().and_then(|rt| rt.snapshot(cstr(name), take != 0)) {
        if cap > 0 && !buf.is_null() {
            let n = text.len().min(cap - 1);
            std::ptr::copy_nonoverlapping(text.as_ptr(), buf as *mut u8, n);
            *buf.add(n) = 0;
        }
        return text.len() as i64;
    }
    forward!("tapir_snapshot", unsafe extern "C" fn(*const c_char, c_int, *mut c_char, usize) -> i64, name, take, buf, cap).unwrap_or(-1)
}

/// The current interval of profiler `name` as a sparse vector: up to `cap`
/// `(ids[i], weights[i])` pairs. Returns the number of pairs, or -1 if no
/// loaded tapir library has a profiler called `name` with a vector form.
///
/// With `take != 0` the interval is closed without tapir writing it. If the
/// vector has more than `cap` entries nothing is taken and the required
/// count is returned, so the caller can grow its buffers and retry.
#[no_mangle]
pub unsafe extern "C" fn tapir_vector(name: *const c_char, take: c_int, ids: *mut u64, weights: *mut f64, cap: usize) -> i64 {
    if let Some(rt) = runtime::get() {
        let cap = if ids.is_null() || weights.is_null() { 0 } else { cap };
        match rt.vector(cstr(name), take != 0, cap) {
            Some(Ok(v)) => {
                for (i, (id, w)) in v.iter().enumerate().take(cap) {
                    *ids.add(i) = *id;
                    *weights.add(i) = *w;
                }
                return v.len() as i64;
            }
            Some(Err(len)) => return len as i64,
            None => {}
        }
    }
    forward!("tapir_vector", unsafe extern "C" fn(*const c_char, c_int, *mut u64, *mut f64, usize) -> i64, name, take, ids, weights, cap).unwrap_or(-1)
}

/// Deliver a named region marker (begin != 0 for begin) to all profilers.
#[no_mangle]
pub unsafe extern "C" fn tapir_marker(name: *const c_char, begin: c_int) {
    if let Some(rt) = runtime::get() {
        rt.dispatch(&Event::Marker { name: cstr(name), begin: begin != 0, t: sys::now_ns() }, false);
    }
    forward!("tapir_marker", unsafe extern "C" fn(*const c_char, c_int), name, begin);
}

/// Report a call, or any other named activity with a time span, that tapir
/// does not wrap itself: another tool's callback, a manually instrumented
/// region, an asynchronous operation observed elsewhere. Profilers see it as
/// an ordinary call of `func` in library `lib`. `arg_names[i]` may carry a
/// tag after a colon (`"n:size"`). Times are `tapir_now_ns()` values; pass 0
/// for both to timestamp "now".
#[no_mangle]
pub unsafe extern "C" fn tapir_record(
    lib: *const c_char,
    func: *const c_char,
    nargs: c_int,
    arg_names: *const *const c_char,
    arg_values: *const i64,
    t_start: u64,
    t_end: u64,
) {
    if let Some(rt) = runtime::get().filter(|rt| rt.is_enabled()) {
        let n = nargs.max(0) as usize;
        let names: Vec<&str> = (0..n).map(|i| if arg_names.is_null() { "" } else { cstr(*arg_names.add(i)) }).collect();
        let values: Vec<Value> = (0..n).map(|i| if arg_values.is_null() { Value::None } else { Value::Int(*arg_values.add(i)) }).collect();
        // "n:size" tags the argument; a bare name is untagged.
        let args: Vec<(&str, &str)> = names.iter().map(|n| n.split_once(':').unwrap_or((n, ""))).collect();
        let spec = FnSpec::interned(cstr(lib), cstr(func), &args);
        let (t0, t1) = if t_start == 0 && t_end == 0 { let t = sys::now_ns(); (t, t) } else { (t_start, t_end) };
        let c = Call { spec, args: &values, t_start: t0, t_end: t1, depth: 0, thread: runtime::thread_index() };
        rt.dispatch(&Event::Call(&c), false);
    }
    forward!(
        "tapir_record",
        unsafe extern "C" fn(*const c_char, *const c_char, c_int, *const *const c_char, *const i64, u64, u64),
        lib, func, nargs, arg_names, arg_values, t_start, t_end
    );
}

/// Write end-of-run output now (also happens automatically at exit).
#[no_mangle]
pub extern "C" fn tapir_flush() {
    if let Some(rt) = runtime::get() {
        rt.finish();
    }
    forward!("tapir_flush", extern "C" fn(),);
}

/// tapir's clock (CLOCK_MONOTONIC ns), for timestamps passed to `tapir_record`.
#[no_mangle]
pub extern "C" fn tapir_now_ns() -> u64 {
    sys::now_ns()
}
