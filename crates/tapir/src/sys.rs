//! The handful of libc symbols tapir needs, declared directly so the crate
//! has no dependencies.

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

extern "C" {
    fn clock_gettime(clk: c_int, ts: *mut Timespec) -> c_int;
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    pub fn atexit(f: extern "C" fn()) -> c_int;
}

/// glibc's `RTLD_NEXT`.
pub const RTLD_NEXT: *mut c_void = -1isize as *mut c_void;
/// glibc's `RTLD_DEFAULT`.
pub const RTLD_DEFAULT: *mut c_void = std::ptr::null_mut();

const CLOCK_MONOTONIC: c_int = 1;

/// `CLOCK_MONOTONIC` in nanoseconds: the clock most tracing tools use, so
/// timestamps reported by other tools line up.
#[inline]
pub fn now_ns() -> u64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Name of the code at `addr`: `symbol`, `symbol+0x10`, or, for code with no
/// dynamic symbol (static functions), `object+0x1234` relative to the
/// object's load base. Stable across runs despite ASLR. Cached per address.
pub fn symbol_name(addr: usize) -> String {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&addr) {
        return s.clone();
    }
    let mut info = DlInfo { dli_fname: std::ptr::null(), dli_fbase: std::ptr::null_mut(), dli_sname: std::ptr::null(), dli_saddr: std::ptr::null_mut() };
    let cstr = |p: *const c_char| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned();
    let name = if unsafe { dladdr(addr as *const c_void, &mut info) } == 0 {
        format!("{addr:#x}")
    } else if !info.dli_sname.is_null() {
        let off = addr - info.dli_saddr as usize;
        if off == 0 { cstr(info.dli_sname) } else { format!("{}+{off:#x}", cstr(info.dli_sname)) }
    } else if !info.dli_fname.is_null() {
        let file = cstr(info.dli_fname);
        let base = file.rsplit('/').next().unwrap_or(&file).to_owned();
        format!("{}+{:#x}", if base.is_empty() { "?" } else { &base }, addr - info.dli_fbase as usize)
    } else {
        format!("{addr:#x}")
    };
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(addr, name.clone());
    name
}
