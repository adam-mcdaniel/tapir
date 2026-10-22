//! A tapir preload library for a few C standard library functions, written by
//! hand with `interpose!` (no generator, no shared vocabulary). It shows
//! string (`cstr`) and callback (`sym`) capture, and tags chosen for this API
//! alone:
//!
//! ```sh
//! TAPIR_PROFILERS=summary,trace LD_PRELOAD=libtapir_posixio.so ./app
//! ```
//!
//! The default key groups transfers by size class (`@bytes:log2`) and
//! element count, and keeps file names, open modes and comparators exact.

use std::ffi::{c_char, c_int, c_void};

tapir::preload!(|b| {
    b.default_key("@bytes:log2; @count:log2");
    b.from_env("bbv,summary");
});

tapir::interpose! {
    lib "stdio";
    fn fopen(path: *const c_char => cstr "path", mode: *const c_char => cstr "mode") -> *mut c_void;
    fn fclose(f: *mut c_void => skip) -> c_int;
    fn fread(ptr: *mut c_void => skip, size: usize => val "bytes", n: usize => val "count", f: *mut c_void => skip) -> usize;
    fn fwrite(ptr: *const c_void => skip, size: usize => val "bytes", n: usize => val "count", f: *mut c_void => skip) -> usize;

    lib "stdlib";
    fn qsort(base: *mut c_void => skip, n: usize => val "count", size: usize => val "bytes", compar: *const c_void => sym);
}
