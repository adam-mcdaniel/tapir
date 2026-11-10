//! CUDA runtime calls that do work outside the math libraries: allocation and
//! freeing (device, pinned host, managed), copies, memsets and kernel launches.
//! The counterpart of tapir-rocm's `hip.rs`, call for call: sizes are tagged
//! `dim` (bucketed by bit length like the math libraries' sizes), copy
//! directions and allocation flags `flag` (exact). Calls made from inside a
//! wrapped library are nested and, like every nested call, not recorded by
//! default. (The CUDA math libraries carry their own copy of the runtime, so
//! their internal launches do not come through here at all.)
//!
//! Functions that take a stream also exist as `<name>_ptsz`, which is what a
//! program built with `CUDA_API_PER_THREAD_DEFAULT_STREAM` calls; both are
//! wrapped and recorded under the plain name's arguments.
//!
//! Signatures are those of `cuda_runtime_api.h`, unchanged from CUDA 9 to 12.9.

use std::ffi::c_void;

use tapir::{ArgSpec, FnSpec, Value};

/// Declare each wrapper twice: as written, and as `<name>_ptsz`.
macro_rules! with_ptsz {
    ( $( fn $name:ident / $ptsz:ident ( $($args:tt)* ) -> i32; )* ) => {
        tapir::interpose! {
            lib "cuda";
            $( fn $name( $($args)* ) -> i32; fn $ptsz( $($args)* ) -> i32; )*
        }
    };
}

tapir::interpose! {
    lib "cuda";
    // allocation / free
    fn cudaMalloc(ptr: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn cudaMallocManaged(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn cudaMallocPitch(ptr: *mut c_void => skip, pitch: *mut c_void => skip, width: usize => val "dim", height: usize => val "dim") -> i32;
    fn cudaMallocHost(ptr: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn cudaHostAlloc(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn cudaHostRegister(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn cudaFree(ptr: *mut c_void => skip) -> i32;
    fn cudaFreeHost(ptr: *mut c_void => skip) -> i32;
    fn cudaHostUnregister(ptr: *mut c_void => skip) -> i32;
    // copies
    fn cudaMemcpy(dst: *mut c_void => skip, src: *const c_void => skip, size: usize => val "dim", kind: i32 => val "flag") -> i32;
    fn cudaMemcpy2D(dst: *mut c_void => skip, dpitch: usize => skip, src: *const c_void => skip, spitch: usize => skip,
                    width: usize => val "dim", height: usize => val "dim", kind: i32 => val "flag") -> i32;
    fn cudaMemcpyToSymbol(symbol: *const c_void => skip, src: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag") -> i32;
    fn cudaMemcpyFromSymbol(dst: *mut c_void => skip, symbol: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag") -> i32;
    fn cudaMemcpyPeer(dst: *mut c_void => skip, dst_device: i32 => skip, src: *const c_void => skip, src_device: i32 => skip, size: usize => val "dim") -> i32;
    fn cudaMemcpyPeerAsync(dst: *mut c_void => skip, dst_device: i32 => skip, src: *const c_void => skip, src_device: i32 => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    // memsets
    fn cudaMemset(dst: *mut c_void => skip, value: i32 => skip, size: usize => val "dim") -> i32;
    fn cudaMemset2D(dst: *mut c_void => skip, pitch: usize => skip, value: i32 => skip, width: usize => val "dim", height: usize => val "dim") -> i32;
}

with_ptsz! {
    fn cudaMallocAsync / cudaMallocAsync_ptsz(ptr: *mut c_void => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn cudaMallocFromPoolAsync / cudaMallocFromPoolAsync_ptsz(ptr: *mut c_void => skip, size: usize => val "dim", pool: *mut c_void => skip, stream: *mut c_void => skip) -> i32;
    fn cudaFreeAsync / cudaFreeAsync_ptsz(ptr: *mut c_void => skip, stream: *mut c_void => skip) -> i32;
    fn cudaMemcpyAsync / cudaMemcpyAsync_ptsz(dst: *mut c_void => skip, src: *const c_void => skip, size: usize => val "dim", kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn cudaMemcpy2DAsync / cudaMemcpy2DAsync_ptsz(dst: *mut c_void => skip, dpitch: usize => skip, src: *const c_void => skip, spitch: usize => skip,
                         width: usize => val "dim", height: usize => val "dim", kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn cudaMemcpyToSymbolAsync / cudaMemcpyToSymbolAsync_ptsz(symbol: *const c_void => skip, src: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn cudaMemcpyFromSymbolAsync / cudaMemcpyFromSymbolAsync_ptsz(dst: *mut c_void => skip, symbol: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn cudaMemsetAsync / cudaMemsetAsync_ptsz(dst: *mut c_void => skip, value: i32 => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn cudaMemset2DAsync / cudaMemset2DAsync_ptsz(dst: *mut c_void => skip, pitch: usize => skip, value: i32 => skip, width: usize => val "dim", height: usize => val "dim", stream: *mut c_void => skip) -> i32;
}

/// CUDA's `dim3`, passed by value.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dim3 {
    x: u32,
    y: u32,
    z: u32,
}

impl Dim3 {
    fn total(self) -> i64 {
        self.x as i64 * self.y as i64 * self.z as i64
    }
}

/// The head of `cudaLaunchConfig_t`; the stream and attributes that follow are
/// not read.
#[repr(C)]
pub struct LaunchConfig {
    grid: Dim3,
    block: Dim3,
}

/// Launch arguments every kernel-launch wrapper records: the kernel (its host
/// stub, named by symbol or `binary+offset`), total blocks and threads per
/// block. The same three as tapir-rocm's launch wrappers.
const LAUNCH_ARGS: &[ArgSpec] = &[ArgSpec::new("kernel", "flag"), ArgSpec::new("blocks", "dim"), ArgSpec::new("threads", "dim")];

macro_rules! launch_wrapper {
    ($name:ident, $cname:literal, ( $( $arg:ident : $ty:ty ),* ), $f:ident, $grid:expr, $block:expr) => {
        #[no_mangle]
        #[allow(non_snake_case, clippy::too_many_arguments, clippy::missing_safety_doc)]
        pub unsafe extern "C" fn $name( $( $arg : $ty ),* ) -> i32 {
            static SPEC: FnSpec = FnSpec::new("cuda", ::core::stringify!($name), LAUNCH_ARGS);
            static REAL: tapir::interpose::RealFn = tapir::interpose::RealFn::new($cname);
            type Real = unsafe extern "C" fn( $( $ty ),* ) -> i32;
            let real: Real = std::mem::transmute::<*mut c_void, Real>(REAL.get());
            let (grid, block): (Dim3, Dim3) = ($grid, $block);
            let args = [Value::Sym($f as usize), Value::Int(grid.total()), Value::Int(block.total())];
            tapir::call(&SPEC, &args, move || real( $( $arg ),* ))
        }
    };
}

/// `(func, grid, block, args, shared_bytes, stream)`: what `kernel<<<...>>>(...)`
/// compiles to, with nvcc and with clang.
macro_rules! launch_dims {
    ($name:ident, $cname:literal) => {
        launch_wrapper!($name, $cname,
            (f: *const c_void, grid: Dim3, block: Dim3, args: *mut *mut c_void, shared_bytes: usize, stream: *mut c_void), f, grid, block);
    };
}

/// `(config, func, args)`: C++ `cudaLaunchKernelEx`.
macro_rules! launch_config {
    ($name:ident, $cname:literal) => {
        launch_wrapper!($name, $cname, (config: *const LaunchConfig, f: *const c_void, args: *mut *mut c_void), f,
            if config.is_null() { Dim3 { x: 0, y: 0, z: 0 } } else { (*config).grid },
            if config.is_null() { Dim3 { x: 0, y: 0, z: 0 } } else { (*config).block });
    };
}

launch_dims!(cudaLaunchKernel, c"cudaLaunchKernel");
launch_dims!(cudaLaunchKernel_ptsz, c"cudaLaunchKernel_ptsz");
launch_dims!(cudaLaunchCooperativeKernel, c"cudaLaunchCooperativeKernel");
launch_dims!(cudaLaunchCooperativeKernel_ptsz, c"cudaLaunchCooperativeKernel_ptsz");
launch_config!(cudaLaunchKernelExC, c"cudaLaunchKernelExC");
launch_config!(cudaLaunchKernelExC_ptsz, c"cudaLaunchKernelExC_ptsz");
