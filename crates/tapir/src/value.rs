//! The argument data model: what a captured argument *is* ([`Value`]) and the
//! static description of a wrapped function ([`FnSpec`], [`ArgSpec`]).
//!
//! tapir attaches no meaning to arguments. A wrapper may give an argument a
//! free-form *tag* (any string, e.g. `"size"`, `"path"`, `"rank"`); tags only
//! matter to whoever reads them, typically a [`KeyPolicy`](crate::KeyPolicy)
//! rule such as `@size:log2`.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

/// A captured argument value. `Copy` and heap-free, so capturing costs a few
/// register moves. `Str` borrows the caller's memory and is valid only while
/// the event is being delivered; copy it if you need to keep it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<'a> {
    /// Not captured (`skip`, or a null pointer behind `deref`/`cstr`).
    None,
    Int(i64),
    UInt(u64),
    Float(f64),
    /// A C `char` read as a character (`chr` / `deref_chr` capture).
    Char(u8),
    /// A pointer recorded by address, never dereferenced.
    Ptr(usize),
    /// A code address (`sym` capture), e.g. a callback. Rendered as its symbol
    /// name, or `object+offset`, so it is stable across runs despite ASLR.
    Sym(usize),
    /// A NUL-terminated string (`cstr` capture), without the NUL.
    Str(&'a [u8]),
}

impl Value<'_> {
    /// Integer view, if this value is integral.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::UInt(v) => Some(v as i64),
            Value::Char(c) => Some(c as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Int(v) => Some(v as f64),
            Value::UInt(v) => Some(v as f64),
            Value::Float(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<std::borrow::Cow<'_, str>> {
        match self {
            Value::Str(s) => Some(String::from_utf8_lossy(s)),
            _ => None,
        }
    }
}

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Value::None => f.write_str("_"),
            Value::Int(v) => write!(f, "{v}"),
            Value::UInt(v) => write!(f, "{v}"),
            // -0.0 and 0.0 must print identically or they split a signature.
            Value::Float(v) => write!(f, "{:e}", if v == 0.0 { 0.0 } else { v }),
            Value::Char(c) if c.is_ascii_graphic() => write!(f, "{}", c as char),
            Value::Char(c) => write!(f, "\\x{c:02x}"),
            Value::Ptr(p) => write!(f, "{p:#x}"),
            Value::Sym(p) => f.write_str(&crate::sys::symbol_name(p)),
            Value::Str(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
        }
    }
}

/// One argument of a wrapped function: its C parameter name and optional tag
/// (`""` for none).
#[derive(Debug)]
pub struct ArgSpec {
    pub name: &'static str,
    pub tag: &'static str,
}

impl ArgSpec {
    pub const fn new(name: &'static str, tag: &'static str) -> Self {
        ArgSpec { name, tag }
    }
}

/// Static description of one function. One lives in a `static` inside every
/// generated wrapper, so `&'static FnSpec` doubles as a cheap identity.
#[derive(Debug)]
pub struct FnSpec {
    /// Library tag, free-form; used for filtering and in output.
    pub lib: &'static str,
    pub name: &'static str,
    pub args: &'static [ArgSpec],
    /// Process-unique dense id, assigned on first use (0 = unassigned).
    id: AtomicU32,
}

static NEXT_FN_ID: AtomicU32 = AtomicU32::new(1);

impl FnSpec {
    pub const fn new(lib: &'static str, name: &'static str, args: &'static [ArgSpec]) -> Self {
        FnSpec { lib, name, args, id: AtomicU32::new(0) }
    }

    /// Dense id in `1..`, stable for the life of the process (assignment order
    /// is first-call order, so do not persist it across runs; use `name`).
    pub fn id(&self) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id != 0 {
            return id;
        }
        let fresh = NEXT_FN_ID.fetch_add(1, Ordering::Relaxed);
        match self.id.compare_exchange(0, fresh, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => fresh,
            Err(existing) => existing,
        }
    }

    /// A process-lifetime spec for a function only known at runtime (calls
    /// reported through `tapir_record`). Deduplicated, so the same
    /// `(lib, name, args)` always yields the same `&'static FnSpec`.
    pub fn interned(lib: &str, name: &str, args: &[(&str, &str)]) -> &'static FnSpec {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static SPECS: OnceLock<Mutex<HashMap<String, &'static FnSpec>>> = OnceLock::new();
        let mut key = format!("{lib}\x1f{name}");
        for (a, t) in args {
            key.push('\x1f');
            key.push_str(a);
            key.push('\x1e');
            key.push_str(t);
        }
        let mut m = SPECS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        m.entry(key).or_insert_with(|| {
            let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
            let args: Vec<ArgSpec> = args.iter().map(|(a, t)| ArgSpec::new(leak(a), leak(t))).collect();
            Box::leak(Box::new(FnSpec::new(leak(lib), leak(name), Box::leak(args.into_boxed_slice()))))
        })
    }
}
