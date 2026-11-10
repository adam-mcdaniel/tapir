//! `libtapir_rocm.so`: LD_PRELOAD it to profile rocBLAS / rocSOLVER /
//! hipBLAS / hipSOLVER / rocSPARSE calls, and the HIP runtime's allocation,
//! copy, memset and kernel-launch calls (`hip.rs`).
//!
//! ```sh
//! TAPIR_PROFILERS="bbv,summary" LD_PRELOAD=libtapir_rocm.so ./app
//! ```
//!
//! Extra runtime option: `TAPIR_GPU_SYNC=device` calls `hipDeviceSynchronize`
//! after each recorded call so durations cover GPU execution, not just the
//! asynchronous launch (adds overhead; only needed for time-weighted output).

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

static HIP_SYNC: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn hip_device_sync() {
    let mut p = HIP_SYNC.load(Ordering::Relaxed);
    if p.is_null() {
        p = tapir::interpose::lookup_global(c"hipDeviceSynchronize");
        if p.is_null() {
            return;
        }
        HIP_SYNC.store(p, Ordering::Relaxed);
    }
    let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
    unsafe { f() };
}

tapir::preload!(|b| {
    b.default_key(tapir_vocab::LINALG.key);
    b.from_env("bbv,summary");
    if std::env::var("TAPIR_GPU_SYNC").is_ok_and(|v| v == "device" || v == "1") {
        b.after_call(hip_device_sync);
    }
});

include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));

mod hip;
