//! `libtapir_cuda.so`: LD_PRELOAD it to profile cuBLAS / cuSOLVER / cuSPARSE
//! calls, and the CUDA runtime's allocation, copy, memset and kernel-launch
//! calls (`cudart.rs`). The NVIDIA counterpart of `libtapir_rocm.so`: same
//! vocabulary, same key rules, same profilers and output.
//!
//! ```sh
//! TAPIR_PROFILERS="bbv,summary" LD_PRELOAD=libtapir_cuda.so ./app
//! ```
//!
//! Extra runtime option: `TAPIR_GPU_SYNC=device` calls `cudaDeviceSynchronize`
//! after each recorded call so durations cover GPU execution, not just the
//! asynchronous launch (adds overhead; only needed for time-weighted output).

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

static CUDA_SYNC: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn cuda_device_sync() {
    let mut p = CUDA_SYNC.load(Ordering::Relaxed);
    if p.is_null() {
        p = tapir::interpose::lookup_global(c"cudaDeviceSynchronize");
        if p.is_null() {
            return;
        }
        CUDA_SYNC.store(p, Ordering::Relaxed);
    }
    let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
    unsafe { f() };
}

tapir::preload!(|b| {
    b.default_key(tapir_vocab::LINALG.key);
    b.from_env("bbv,summary");
    if std::env::var("TAPIR_GPU_SYNC").is_ok_and(|v| v == "device" || v == "1") {
        b.after_call(cuda_device_sync);
    }
});

include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));

mod cudart;
