//! HIP runtime calls that do work outside the math libraries: allocation and
//! freeing (device, pinned host, managed), copies, memsets and kernel launches.
//! Sizes are tagged `dim` (bucketed by bit length like the math libraries'
//! sizes), copy directions and allocation flags `flag` (exact). Calls made from
//! inside a wrapped library (e.g. rocBLAS launching its own kernels) are nested
//! and, like every nested call, not recorded by default.

use std::ffi::c_void;

use tapir::{ArgSpec, FnSpec, Value};

tapir::interpose! {
    lib "hip";
    // allocation / free
    fn hipMalloc(ptr: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn hipMallocManaged(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn hipMallocAsync(ptr: *mut c_void => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipExtMallocWithFlags(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn hipHostMalloc(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn hipMallocHost(ptr: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn hipHostAlloc(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn hipHostRegister(ptr: *mut c_void => skip, size: usize => val "dim", flags: u32 => val "flag") -> i32;
    fn hipFree(ptr: *mut c_void => skip) -> i32;
    fn hipHostFree(ptr: *mut c_void => skip) -> i32;
    fn hipFreeHost(ptr: *mut c_void => skip) -> i32;
    fn hipFreeAsync(ptr: *mut c_void => skip, stream: *mut c_void => skip) -> i32;
    fn hipHostUnregister(ptr: *mut c_void => skip) -> i32;
    // copies
    fn hipMemcpy(dst: *mut c_void => skip, src: *const c_void => skip, size: usize => val "dim", kind: i32 => val "flag") -> i32;
    fn hipMemcpyAsync(dst: *mut c_void => skip, src: *const c_void => skip, size: usize => val "dim", kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyWithStream(dst: *mut c_void => skip, src: *const c_void => skip, size: usize => val "dim", kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyHtoD(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn hipMemcpyDtoH(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn hipMemcpyDtoD(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim") -> i32;
    fn hipMemcpyHtoDAsync(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyDtoHAsync(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyDtoDAsync(dst: *mut c_void => skip, src: *mut c_void => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemcpy2D(dst: *mut c_void => skip, dpitch: usize => skip, src: *const c_void => skip, spitch: usize => skip,
                   width: usize => val "dim", height: usize => val "dim", kind: i32 => val "flag") -> i32;
    fn hipMemcpy2DAsync(dst: *mut c_void => skip, dpitch: usize => skip, src: *const c_void => skip, spitch: usize => skip,
                        width: usize => val "dim", height: usize => val "dim", kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyToSymbol(symbol: *const c_void => skip, src: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag") -> i32;
    fn hipMemcpyToSymbolAsync(symbol: *const c_void => skip, src: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyFromSymbol(dst: *mut c_void => skip, symbol: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag") -> i32;
    fn hipMemcpyFromSymbolAsync(dst: *mut c_void => skip, symbol: *const c_void => skip, size: usize => val "dim", offset: usize => skip, kind: i32 => val "flag", stream: *mut c_void => skip) -> i32;
    fn hipMemcpyPeer(dst: *mut c_void => skip, dst_device: i32 => skip, src: *const c_void => skip, src_device: i32 => skip, size: usize => val "dim") -> i32;
    fn hipMemcpyPeerAsync(dst: *mut c_void => skip, dst_device: i32 => skip, src: *const c_void => skip, src_device: i32 => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    // memsets
    fn hipMemset(dst: *mut c_void => skip, value: i32 => skip, size: usize => val "dim") -> i32;
    fn hipMemsetAsync(dst: *mut c_void => skip, value: i32 => skip, size: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemset2D(dst: *mut c_void => skip, pitch: usize => skip, value: i32 => skip, width: usize => val "dim", height: usize => val "dim") -> i32;
    fn hipMemset2DAsync(dst: *mut c_void => skip, pitch: usize => skip, value: i32 => skip, width: usize => val "dim", height: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemsetD32(dst: *mut c_void => skip, value: i32 => skip, count: usize => val "dim") -> i32;
    fn hipMemsetD32Async(dst: *mut c_void => skip, value: i32 => skip, count: usize => val "dim", stream: *mut c_void => skip) -> i32;
    fn hipMemsetD8(dst: *mut c_void => skip, value: u8 => skip, count: usize => val "dim") -> i32;
    fn hipMemsetD8Async(dst: *mut c_void => skip, value: u8 => skip, count: usize => val "dim", stream: *mut c_void => skip) -> i32;
    // module launch: grid and block passed as scalars
    fn hipModuleLaunchKernel(f: *mut c_void => val "flag", grid_x: u32 => val "dim", grid_y: u32 => val "dim", grid_z: u32 => val "dim",
                             block_x: u32 => val "dim", block_y: u32 => val "dim", block_z: u32 => val "dim",
                             shared_bytes: u32 => skip, stream: *mut c_void => skip, params: *mut c_void => skip, extra: *mut c_void => skip) -> i32;
}

/// HIP's `dim3`, passed by value.
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

/// Launch arguments every kernel-launch wrapper records: the kernel (its host
/// stub, named by symbol or `binary+offset`), total blocks and threads per block.
const LAUNCH_ARGS: &[ArgSpec] = &[ArgSpec::new("kernel", "flag"), ArgSpec::new("blocks", "dim"), ArgSpec::new("threads", "dim")];

macro_rules! launch_wrapper {
    ($name:ident, $cname:literal, ( $( $arg:ident : $ty:ty ),* ), $f:ident, $grid:ident, $block:ident) => {
        #[no_mangle]
        #[allow(non_snake_case, clippy::too_many_arguments, clippy::missing_safety_doc)]
        pub unsafe extern "C" fn $name( $( $arg : $ty ),* ) -> i32 {
            static SPEC: FnSpec = FnSpec::new("hip", ::core::stringify!($name), LAUNCH_ARGS);
            static REAL: tapir::interpose::RealFn = tapir::interpose::RealFn::new($cname);
            type Real = unsafe extern "C" fn( $( $ty ),* ) -> i32;
            let real: Real = std::mem::transmute::<*mut c_void, Real>(REAL.get());
            let args = [Value::Sym($f as usize), Value::Int($grid.total()), Value::Int($block.total())];
            tapir::call(&SPEC, &args, move || real( $( $arg ),* ))
        }
    };
}

launch_wrapper!(hipLaunchKernel, c"hipLaunchKernel",
    (f: *const c_void, grid: Dim3, block: Dim3, args: *mut *mut c_void, shared_bytes: usize, stream: *mut c_void), f, grid, block);
launch_wrapper!(hipExtLaunchKernel, c"hipExtLaunchKernel",
    (f: *const c_void, grid: Dim3, block: Dim3, args: *mut *mut c_void, shared_bytes: usize, stream: *mut c_void,
     start: *mut c_void, stop: *mut c_void, flags: i32), f, grid, block);
launch_wrapper!(hipLaunchCooperativeKernel, c"hipLaunchCooperativeKernel",
    (f: *const c_void, grid: Dim3, block: Dim3, args: *mut *mut c_void, shared_bytes: u32, stream: *mut c_void), f, grid, block);
