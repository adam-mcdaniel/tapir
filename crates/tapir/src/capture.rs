//! Capture modes: *how* [`interpose!`](crate::interpose) reads an argument.
//!
//! In a wrapper every argument is written `name: Type => mode` or
//! `name: Type => mode "tag"`. The mode is purely mechanical, and the type
//! checker rejects a mode that does not fit the type:
//!
//! | mode        | reads                                     | C type                    |
//! |-------------|-------------------------------------------|---------------------------|
//! | `val`       | the argument itself                       | integers, floats, pointers (as address) |
//! | `deref`     | the scalar behind a pointer               | `const int *`, `const double *` |
//! | `chr`       | the argument as a character               | `char`                    |
//! | `deref_chr` | the character behind a pointer            | `const char *` (one char) |
//! | `cstr`      | a NUL-terminated string                   | `const char *`            |
//! | `sym`       | a code address, reported by symbol name   | function pointers         |
//! | `skip`      | nothing                                   | anything                  |
//!
//! `deref`, `deref_chr` and `cstr` read memory: use them only where the
//! pointer is guaranteed to be readable host memory. Null pointers are safe
//! and captured as [`Value::None`].

use std::ffi::c_void;

use crate::value::Value;

/// Read by value.
pub trait ByValue {
    fn by_value(&self) -> Value<'static>;
}

/// Read the scalar a pointer points to.
pub trait ByRef {
    fn by_ref(&self) -> Value<'static>;
}

/// A C `char`.
pub trait AsChar {
    fn as_char(&self) -> Value<'static>;
}

/// A pointer to a C `char`.
pub trait DerefChar {
    fn deref_char(&self) -> Value<'static>;
}

/// A pointer to a NUL-terminated string.
pub trait AsCStr {
    fn as_cstr(&self) -> Value<'static>;
}

/// A code address.
pub trait AsSym {
    fn as_sym(&self) -> Value<'static>;
}

macro_rules! scalars {
    ($($t:ty => $variant:ident as $cast:ty),* $(,)?) => {$(
        impl ByValue for $t {
            #[inline(always)]
            fn by_value(&self) -> Value<'static> { Value::$variant(*self as $cast) }
        }
        impl ByRef for *const $t {
            #[inline(always)]
            fn by_ref(&self) -> Value<'static> {
                if self.is_null() { Value::None } else { Value::$variant(unsafe { **self } as $cast) }
            }
        }
        impl ByRef for *mut $t {
            #[inline(always)]
            fn by_ref(&self) -> Value<'static> { (*self as *const $t).by_ref() }
        }
    )*};
}

scalars! {
    i8 => Int as i64, i16 => Int as i64, i32 => Int as i64, i64 => Int as i64, isize => Int as i64,
    u8 => UInt as u64, u16 => UInt as u64, u32 => UInt as u64, u64 => UInt as u64, usize => UInt as u64,
    f32 => Float as f64, f64 => Float as f64,
}

// Any pointer, by value, is its address.
impl<T: ?Sized> ByValue for *const T {
    #[inline(always)]
    fn by_value(&self) -> Value<'static> {
        Value::Ptr(*self as *const u8 as usize)
    }
}
impl<T: ?Sized> ByValue for *mut T {
    #[inline(always)]
    fn by_value(&self) -> Value<'static> {
        Value::Ptr(*self as *const u8 as usize)
    }
}

macro_rules! chars {
    ($($t:ty),*) => {$(
        impl AsChar for $t {
            #[inline(always)]
            fn as_char(&self) -> Value<'static> { Value::Char(*self as u8) }
        }
        impl DerefChar for *const $t {
            #[inline(always)]
            fn deref_char(&self) -> Value<'static> {
                if self.is_null() { Value::None } else { Value::Char(unsafe { **self } as u8) }
            }
        }
        impl DerefChar for *mut $t {
            #[inline(always)]
            fn deref_char(&self) -> Value<'static> { (*self as *const $t).deref_char() }
        }
        impl AsCStr for *const $t {
            #[inline(always)]
            fn as_cstr(&self) -> Value<'static> {
                if self.is_null() {
                    return Value::None;
                }
                // SAFETY: the wrapper declaration promises a readable,
                // NUL-terminated string. The 'static lifetime is narrowed to
                // the event by `Call`, which only lends `&'a [Value<'a>]`.
                let s = unsafe { std::ffi::CStr::from_ptr(*self as *const std::ffi::c_char) };
                Value::Str(s.to_bytes())
            }
        }
        impl AsCStr for *mut $t {
            #[inline(always)]
            fn as_cstr(&self) -> Value<'static> { (*self as *const $t).as_cstr() }
        }
    )*};
}
chars!(i8, u8);

impl AsCStr for *const c_void {
    #[inline(always)]
    fn as_cstr(&self) -> Value<'static> {
        (*self as *const u8).as_cstr()
    }
}

impl AsSym for *const c_void {
    #[inline(always)]
    fn as_sym(&self) -> Value<'static> {
        if self.is_null() { Value::None } else { Value::Sym(*self as usize) }
    }
}
impl AsSym for *mut c_void {
    #[inline(always)]
    fn as_sym(&self) -> Value<'static> {
        (*self as *const c_void).as_sym()
    }
}
impl AsSym for usize {
    #[inline(always)]
    fn as_sym(&self) -> Value<'static> {
        (*self as *const c_void).as_sym()
    }
}

/// One module per mode, so the macro can write `$crate::capture::$mode::capture(&arg)`.
macro_rules! modes {
    ($($mode:ident => $tr:ident :: $method:ident;)*) => {$(
        pub mod $mode {
            #[inline(always)]
            pub fn capture<T: super::$tr>(v: &T) -> $crate::Value<'static> { v.$method() }
        }
    )*};
}

modes! {
    val       => ByValue::by_value;
    deref     => ByRef::by_ref;
    chr       => AsChar::as_char;
    deref_chr => DerefChar::deref_char;
    cstr      => AsCStr::as_cstr;
    sym       => AsSym::as_sym;
}

/// Not captured: the argument is never read.
pub mod skip {
    #[inline(always)]
    pub fn capture<T>(_: &T) -> crate::Value<'static> {
        crate::Value::None
    }
}
