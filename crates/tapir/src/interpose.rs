//! Symbol interposition: the [`interpose!`] macro and real-symbol lookup.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::sys;

/// Lazily resolved pointer to the *next* definition of a symbol (the real
/// library function behind our wrapper). Resolved once, then a relaxed load.
pub struct RealFn {
    name: &'static CStr,
    ptr: AtomicPtr<c_void>,
}

impl RealFn {
    pub const fn new(name: &'static CStr) -> Self {
        RealFn { name, ptr: AtomicPtr::new(std::ptr::null_mut()) }
    }

    #[inline(always)]
    pub fn get(&self) -> *mut c_void {
        let p = self.ptr.load(Ordering::Relaxed);
        if !p.is_null() {
            return p;
        }
        self.resolve()
    }

    #[cold]
    fn resolve(&self) -> *mut c_void {
        let p = unsafe { sys::dlsym(sys::RTLD_NEXT, self.name.as_ptr() as *const c_char) };
        if p.is_null() {
            // The application called a function that only we define. There
            // is nothing sensible to forward to.
            eprintln!("[tapir] fatal: no definition of `{}` after the interposer", self.name.to_string_lossy());
            std::process::abort();
        }
        self.ptr.store(p, Ordering::Relaxed);
        p
    }
}

/// Define interposing wrappers for C functions.
///
/// ```ignore
/// use std::ffi::{c_char, c_void};
/// tapir::interpose! {
///     lib "libc";
///     fn fopen(path: *const c_char => cstr "path", mode: *const c_char => cstr "mode") -> *mut c_void;
///     fn fwrite(ptr: *const c_void => skip, size: usize => val "bytes",
///               n: usize => val "count", f: *mut c_void => skip) -> usize;
///     fn qsort(base: *mut c_void => skip, n: usize => val "count", size: usize => val "bytes",
///              compar: *const c_void => sym);
///
///     lib "flib";
///     fn solve_(n: *const i32 => deref "size", job: *const c_char => deref_chr);
/// }
/// ```
///
/// Each argument is `name: Type => mode` or `name: Type => mode "tag"`. The
/// [mode](crate::capture) says how to read it (`val`, `deref`, `chr`,
/// `deref_chr`, `cstr`, `sym`, `skip`); the optional tag is a free-form label
/// that key rules and profilers can select on (`@bytes:log2`).
///
/// Each `fn` becomes an exported `extern "C"` symbol that captures its
/// arguments, calls the next definition of the same symbol (found with
/// `dlsym(RTLD_NEXT)`), and reports a [`Call`](crate::Call) to the active
/// profilers. Types must be ABI-identical to the C declaration; any C
/// pointer can be written `*const c_void`, and C enums are usually `i32`.
/// `lib "<name>";` sets the library tag for the functions that follow it.
#[macro_export]
macro_rules! interpose {
    ( $(
        lib $lib:literal ;
        $(
            $(#[$meta:meta])*
            fn $name:ident ( $( $arg:ident : $ty:ty => $mode:ident $($tag:literal)? ),* $(,)? ) $( -> $ret:ty )? ;
        )*
    )* ) => {
        $( $(
            $crate::__interpose_one!($lib; $(#[$meta])* fn $name( $( $arg : $ty => $mode $($tag)? ),* ) $( -> $ret )?);
        )* )*
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __interpose_one {
    ($lib:literal; $(#[$meta:meta])* fn $name:ident ( $( $arg:ident : $ty:ty => $mode:ident $($tag:literal)? ),* ) $( -> $ret:ty )?) => {
        $(#[$meta])*
        #[no_mangle]
        #[allow(non_snake_case, clippy::too_many_arguments, clippy::missing_safety_doc)]
        pub unsafe extern "C" fn $name( $( $arg : $ty ),* ) $( -> $ret )? {
            static SPEC: $crate::FnSpec = $crate::FnSpec::new(
                $lib,
                ::core::stringify!($name),
                &[ $( $crate::ArgSpec::new(::core::stringify!($arg), $crate::__tag!($($tag)?)) ),* ],
            );
            static REAL: $crate::interpose::RealFn = $crate::interpose::RealFn::new(
                // SAFETY: the literal is NUL-terminated and has no interior NUL.
                unsafe { ::core::ffi::CStr::from_bytes_with_nul_unchecked(
                    ::core::concat!(::core::stringify!($name), "\0").as_bytes()) },
            );
            type Real = unsafe extern "C" fn( $( $ty ),* ) $( -> $ret )?;
            let real: Real = ::core::mem::transmute::<*mut ::core::ffi::c_void, Real>(REAL.get());
            let args = [ $( $crate::capture::$mode::capture(&$arg) ),* ];
            $crate::call(&SPEC, &args, move || real( $( $arg ),* ))
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tag {
    () => {
        ""
    };
    ($t:literal) => {
        $t
    };
}

/// Look up a symbol in the global scope (e.g. an optional hook exported by
/// another tool). Returns null if absent.
pub fn lookup_global(name: &CStr) -> *mut c_void {
    unsafe { sys::dlsym(sys::RTLD_DEFAULT, name.as_ptr()) }
}

/// Look up the next definition of `name` after the object that contains the
/// caller (i.e. after this tapir library).
pub fn lookup_next(name: &CStr) -> *mut c_void {
    unsafe { sys::dlsym(sys::RTLD_NEXT, name.as_ptr()) }
}
