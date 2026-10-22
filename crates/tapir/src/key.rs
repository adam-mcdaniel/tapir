//! Turning a call into a *block key*: which calls count as "the same thing".
//!
//! A [`KeyPolicy`] maps each captured argument to a [`Bucket`] (exact value,
//! log2 bit length, linear bin, dropped, ...) through rules that select
//! arguments by tag, by name, or by function and name. A [`BlockTable`]
//! interns the resulting keys to dense ids. With rule `@size:log2`, calls of
//! one function whose `size`-tagged arguments share bit lengths, and whose
//! other captured arguments are equal, get one block id.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::config::glob;
use crate::event::Call;
use crate::value::{ArgSpec, FnSpec, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bucket {
    /// Keep the exact value.
    Exact,
    /// Bit length of |v| with the sign kept: 0 -> 0, 1 -> 1, 2..3 -> 2,
    /// 4..7 -> 3, ..., i.e. bucket b holds [2^(b-1), 2^b).
    Log2,
    /// Log2 with 2^s sub-buckets per octave (finer than `Log2`).
    Log2Sub(u8),
    /// Round to N significant bits (round-half-up), e.g. `Sig(3)` maps 1000
    /// to 1024 and 700 to 640.
    Sig(u8),
    /// Fixed-width bins: v / width.
    Linear(i64),
    /// Classify as 0, 1, -1 or "other": distinguishes special values
    /// without splitting on every value.
    ZeroOne,
    /// Ignore the argument.
    Drop,
}

impl Bucket {
    pub fn parse(s: &str) -> Option<Bucket> {
        let s = s.trim();
        Some(match s {
            "exact" => Bucket::Exact,
            "log2" | "bits" => Bucket::Log2,
            "zeroone" | "01" => Bucket::ZeroOne,
            "drop" | "ignore" | "none" => Bucket::Drop,
            _ => {
                if let Some(n) = s.strip_prefix("log2/") {
                    Bucket::Log2Sub(n.parse().ok()?)
                } else if let Some(n) = s.strip_prefix("sig/") {
                    Bucket::Sig(n.parse().ok().filter(|b: &u8| (1..63).contains(b))?)
                } else if let Some(n) = s.strip_prefix("linear/") {
                    Bucket::Linear(n.parse().ok().filter(|w: &i64| *w > 0)?)
                } else {
                    return None;
                }
            }
        })
    }

    /// Bucket a value. Returns `None` when the argument should not
    /// contribute to the key. Data pointers never do (their addresses differ
    /// between runs); strings and symbols are always kept exactly.
    #[inline]
    pub fn apply(&self, v: Value) -> Option<i64> {
        self.apply_kind(v).map(|(_, x)| x)
    }

    /// Like [`apply`](Bucket::apply), also returning the value's [`Kind`].
    #[inline]
    pub fn apply_kind(&self, v: Value) -> Option<(Kind, i64)> {
        if matches!(self, Bucket::Drop) {
            return None;
        }
        let x = match v {
            Value::None | Value::Ptr(_) => return None,
            Value::Str(s) => return Some((Kind::Str, intern(s) as i64)),
            Value::Sym(a) => return Some((Kind::Sym, a as i64)),
            Value::Float(f) => return Some((Kind::Num, self.apply_float(f))),
            other => other.as_i64()?,
        };
        Some((Kind::Num, self.apply_int(x)))
    }

    fn apply_int(&self, x: i64) -> i64 {
        match *self {
            Bucket::Exact => x,
            Bucket::Log2 => x.signum() * bit_len(x.unsigned_abs()) as i64,
            Bucket::Log2Sub(s) => x.signum() * log2_sub(x.unsigned_abs(), s) as i64,
            Bucket::Sig(bits) => x.signum() * sig_round(x.unsigned_abs(), bits) as i64,
            Bucket::Linear(w) => x.div_euclid(w),
            Bucket::ZeroOne => match x {
                0 => 0,
                1 => 1,
                -1 => -1,
                _ => 2,
            },
            Bucket::Drop => unreachable!(),
        }
    }

    fn apply_float(&self, f: f64) -> i64 {
        match *self {
            Bucket::Exact => (if f == 0.0 { 0.0f64 } else { f }).to_bits() as i64,
            Bucket::ZeroOne => {
                if f == 0.0 {
                    0
                } else if f == 1.0 {
                    1
                } else if f == -1.0 {
                    -1
                } else {
                    2
                }
            }
            // Sign and binary exponent; zero/NaN collapse to 0.
            _ => {
                if f == 0.0 || !f.is_finite() {
                    0
                } else {
                    let e = f.abs().log2().floor() as i64 + 2048;
                    if f < 0.0 { -e } else { e }
                }
            }
        }
    }

    /// Human-readable rendering of a bucketed value. Never contains `,`, `(`,
    /// `)`, `=` or spaces, so labels stay trivially parseable: ranges are
    /// inclusive `lo..hi`, approximations `~v`, strings percent-escaped.
    pub fn describe(&self, kind: Kind, b: i64, out: &mut String) {
        match kind {
            Kind::Str => return escape(interned(b as u32), out),
            Kind::Sym => return escape(&crate::sys::symbol_name(b as usize), out),
            Kind::Num => {}
        }
        match *self {
            Bucket::Log2 if b != 0 => {
                let (lo, hi) = (1u128 << (b.unsigned_abs() - 1), (1u128 << b.unsigned_abs()) - 1);
                let sign = if b < 0 { "-" } else { "" };
                let _ = write!(out, "{sign}{lo}..{hi}");
            }
            Bucket::Linear(w) => {
                let _ = write!(out, "{}..{}", b * w, (b + 1) * w - 1);
            }
            Bucket::Sig(_) => {
                let _ = write!(out, "~{b}");
            }
            Bucket::ZeroOne => out.push_str(match b {
                0 => "0",
                1 => "1",
                -1 => "-1",
                _ => "other",
            }),
            Bucket::Log2Sub(s) if b != 0 => {
                let _ = write!(out, "~2^{:.2}", (b.unsigned_abs() - 1) as f64 / (1u64 << s) as f64);
            }
            _ => {
                let _ = write!(out, "{b}");
            }
        }
    }
}

fn bit_len(x: u64) -> u32 {
    64 - x.leading_zeros()
}

fn sig_round(a: u64, bits: u8) -> u64 {
    let bits = bits as u32;
    if bit_len(a) <= bits {
        return a;
    }
    let step = 1u64 << (bit_len(a) - bits);
    a.saturating_add(step >> 1) / step * step
}

/// Bit length scaled by 2^s plus the next s mantissa bits.
fn log2_sub(x: u64, s: u8) -> u64 {
    if x == 0 {
        return 0;
    }
    let bl = bit_len(x) as u64;
    let s = s.min(16) as u64;
    let frac = if bl > s { (x >> (bl - 1 - s)) & ((1 << s) - 1) } else { (x << (s - (bl - 1))) & ((1 << s) - 1) };
    ((bl - 1) << s) + frac + 1
}

/// What a bucketed key value encodes, so labels can render it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Num = 0,
    /// An id from the process-wide string table.
    Str = 1,
    /// A code address.
    Sym = 2,
}

impl Kind {
    fn from_bits(b: i64) -> Kind {
        match b {
            1 => Kind::Str,
            2 => Kind::Sym,
            _ => Kind::Num,
        }
    }
}

/// String -> id, and id -> string.
type StringTable = (FxMap<Box<[u8]>, u32>, Vec<&'static str>);

/// Process-wide string table for string-valued keys. Grows with the number
/// of *distinct* strings seen in keyed arguments.
fn strings() -> &'static Mutex<StringTable> {
    static S: OnceLock<Mutex<StringTable>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn intern(s: &[u8]) -> u32 {
    let mut g = strings().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&id) = g.0.get(s) {
        return id;
    }
    let id = g.1.len() as u32;
    g.1.push(Box::leak(String::from_utf8_lossy(s).into_owned().into_boxed_str()));
    g.0.insert(s.into(), id);
    id
}

fn interned(id: u32) -> &'static str {
    strings().lock().unwrap_or_else(|e| e.into_inner()).1.get(id as usize).copied().unwrap_or("?")
}

/// Percent-escape the characters labels reserve.
fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            ',' | '(' | ')' | '=' | '%' | '"' => {
                let _ = write!(out, "%{:02X}", c as u32);
            }
            c if c.is_whitespace() || c.is_control() => {
                let _ = write!(out, "%{:02X}", c as u32 & 0xff);
            }
            c => out.push(c),
        }
    }
}

/// Which arguments a rule applies to.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    /// Every argument (`*`).
    Any,
    /// Arguments with this tag (`@size`; `@` alone is "untagged").
    Tag(String),
    /// The argument with this name (`n`).
    Arg(String),
}

#[derive(Clone, Debug)]
struct Rule {
    /// Function-name glob (`lib_*`), or any function.
    func: Option<String>,
    target: Target,
    bucket: Bucket,
}

impl Rule {
    /// Higher is more specific: a function-qualified rule beats an
    /// unqualified one; a name beats a tag beats `*`.
    fn score(&self, func: &str, arg: &ArgSpec) -> Option<u8> {
        if let Some(f) = &self.func {
            if !glob(f, func) {
                return None;
            }
        }
        let t = match &self.target {
            Target::Any => 0,
            Target::Tag(t) if t == arg.tag => 1,
            Target::Arg(a) if a == arg.name => 2,
            _ => return None,
        };
        Some(t + 4 * self.func.is_some() as u8)
    }
}

/// How each argument contributes to a block key.
///
/// Rules are written `selector:bucket`, joined with `;`:
///
/// | selector     | applies to                                  |
/// |--------------|---------------------------------------------|
/// | `*`          | every argument (the default; initially `exact`) |
/// | `@tag`       | arguments tagged `tag` (`@` = untagged)     |
/// | `name`       | arguments named `name`, in any function     |
/// | `fn.name`    | argument `name` of functions matching glob `fn` |
/// | `fn.@tag`, `fn.*` | likewise, within matching functions    |
///
/// The most specific matching rule wins (function-qualified over not; name
/// over tag over `*`); among equals the last one wins. Buckets: `exact`,
/// `log2`, `log2/N`, `sig/N`, `linear/W`, `zeroone`, `drop`.
///
/// ```
/// use tapir::KeyPolicy;
/// let p = KeyPolicy::parse("@size:log2; @scale:drop; resize.mode:drop", KeyPolicy::exact()).unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct KeyPolicy {
    rules: Vec<Rule>,
    /// Resolved buckets per function, indexed by `FnSpec::id()`.
    cache: Vec<Option<Box<[Bucket]>>>,
}

impl Default for KeyPolicy {
    fn default() -> Self {
        KeyPolicy::exact()
    }
}

impl KeyPolicy {
    /// Every captured argument kept exactly: one key per distinct signature.
    pub fn exact() -> Self {
        KeyPolicy { rules: Vec::new(), cache: Vec::new() }
    }

    /// Only the function name matters.
    pub fn name_only() -> Self {
        KeyPolicy::exact().with("*", Bucket::Drop).unwrap()
    }

    /// Add one rule; see the type docs for selector syntax.
    pub fn with(mut self, selector: &str, bucket: Bucket) -> Result<Self, String> {
        let sel = selector.trim();
        let (func, target) = match sel.split_once('.') {
            Some((f, t)) if !f.is_empty() => (Some(f.to_owned()), t),
            Some(_) => return Err(format!("empty function in selector `{sel}`")),
            None => (None, sel),
        };
        let target = match target {
            "" => return Err(format!("empty selector `{sel}`")),
            "*" => Target::Any,
            t => match t.strip_prefix('@') {
                Some(tag) => Target::Tag(tag.to_owned()),
                None => Target::Arg(t.to_owned()),
            },
        };
        self.rules.push(Rule { func, target, bucket });
        self.cache.clear();
        Ok(self)
    }

    /// Parse `selector:bucket; ...` on top of `base`.
    pub fn parse(spec: &str, base: KeyPolicy) -> Result<KeyPolicy, String> {
        let mut p = base;
        for item in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (sel, b) = item.rsplit_once(':').ok_or_else(|| format!("key rule `{item}` needs `selector:bucket`"))?;
            let bucket = Bucket::parse(b).ok_or_else(|| format!("unknown bucket `{b}` in `{item}`"))?;
            p = p.with(sel, bucket)?;
        }
        Ok(p)
    }

    /// The bucket for argument `arg` of function `func`.
    pub fn bucket_for(&self, func: &str, arg: &ArgSpec) -> Bucket {
        let mut best: Option<(u8, Bucket)> = None;
        for r in &self.rules {
            if let Some(s) = r.score(func, arg) {
                if best.is_none_or(|(b, _)| s >= b) {
                    best = Some((s, r.bucket));
                }
            }
        }
        best.map_or(Bucket::Exact, |(_, b)| b)
    }

    /// Write the key for `call` into `key` as
    /// `[fn_id, (arg_index | kind << 32, bucketed value)*]`.
    pub fn key_into(&mut self, call: &Call, key: &mut Vec<i64>) {
        let id = call.spec.id();
        let slot = id as usize;
        if slot >= self.cache.len() {
            self.cache.resize(slot + 1, None);
        }
        if self.cache[slot].is_none() {
            let resolved = call.spec.args.iter().map(|a| self.bucket_for(call.spec.name, a)).collect();
            self.cache[slot] = Some(resolved);
        }
        let buckets = self.cache[slot].as_deref().unwrap();
        key.clear();
        key.push(id as i64);
        for (i, (b, v)) in buckets.iter().zip(call.args).enumerate() {
            if let Some((kind, x)) = b.apply_kind(*v) {
                key.push(i as i64 | (kind as i64) << 32);
                key.push(x);
            }
        }
    }

    /// The keyed arguments of a key built by [`key_into`](KeyPolicy::key_into):
    /// `(arg, rendered bucket)`.
    pub fn key_args<'k>(&'k self, spec: &'static FnSpec, key: &'k [i64]) -> impl Iterator<Item = (&'static ArgSpec, String)> + 'k {
        key[1..].chunks_exact(2).map(move |pair| {
            let arg = &spec.args[(pair[0] & 0xffff_ffff) as usize];
            let mut s = String::new();
            self.bucket_for(spec.name, arg).describe(Kind::from_bits(pair[0] >> 32), pair[1], &mut s);
            (arg, s)
        })
    }

    /// Human-readable label for a key, e.g. `resize(mode=2,w=512..1023)`.
    pub fn describe(&self, spec: &'static FnSpec, key: &[i64]) -> String {
        let mut s = String::from(spec.name);
        s.push('(');
        for (n, (arg, v)) in self.key_args(spec, key).enumerate() {
            if n > 0 {
                s.push(',');
            }
            s.push_str(arg.name);
            s.push('=');
            s.push_str(&v);
        }
        s.push(')');
        s
    }
}

/// FxHash: fast, non-cryptographic; keys here are tiny integer slices.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // `[i64]` hashes through here as one byte slice: consume words.
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.write_u64(u64::from_le_bytes(c.try_into().unwrap()));
        }
        for &b in chunks.remainder() {
            self.write_u64(b as u64);
        }
    }
    #[inline]
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_i64(&mut self, x: i64) {
        self.write_u64(x as u64)
    }
    #[inline]
    fn write_usize(&mut self, x: usize) {
        self.write_u64(x as u64)
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// Interns keys to dense ids `0..len()` in first-seen order.
#[derive(Default)]
pub struct BlockTable {
    ids: FxMap<Box<[i64]>, u32>,
    /// Per id: (function, key) for labelling at output time.
    entries: Vec<(&'static crate::FnSpec, Box<[i64]>)>,
}

impl BlockTable {
    /// Id for `key`; allocates only the first time a key is seen.
    pub fn intern(&mut self, spec: &'static crate::FnSpec, key: &[i64]) -> u32 {
        if let Some(&id) = self.ids.get(key) {
            return id;
        }
        let id = self.entries.len() as u32;
        let boxed: Box<[i64]> = key.into();
        self.entries.push((spec, boxed.clone()));
        self.ids.insert(boxed, id);
        id
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, id: u32) -> (&'static crate::FnSpec, &[i64]) {
        let (s, k) = &self.entries[id as usize];
        (s, k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log2_buckets() {
        let b = Bucket::Log2;
        assert_eq!(b.apply(Value::Int(0)), Some(0));
        assert_eq!(b.apply(Value::Int(1)), Some(1));
        assert_eq!(b.apply(Value::Int(3)), Some(2));
        assert_eq!(b.apply(Value::Int(512)), Some(10));
        assert_eq!(b.apply(Value::Int(1023)), Some(10));
        assert_eq!(b.apply(Value::Int(1024)), Some(11));
        assert_eq!(b.apply(Value::Int(-4)), Some(-3));
        assert_eq!(b.apply(Value::Ptr(0x1000)), None);
        let mut s = String::new();
        b.describe(Kind::Num, 10, &mut s);
        assert_eq!(s, "512..1023");
    }

    #[test]
    fn sig_rounding() {
        // Reference values from a Python implementation of the same rounding.
        let b = Bucket::Sig(3);
        for (v, want) in [(5, 5), (7, 7), (8, 8), (9, 10), (111, 112), (112, 112), (700, 640), (1000, 1024), (79968, 81920), (80032, 81920)] {
            assert_eq!(b.apply(Value::Int(v)), Some(want), "v={v}");
        }
    }

    #[test]
    fn log2_sub_is_monotone() {
        let mut last = 0;
        for x in 1..5000u64 {
            let v = log2_sub(x, 2);
            assert!(v >= last, "x={x}");
            last = v;
        }
        assert_eq!(log2_sub(4, 2), log2_sub(4, 2));
        assert_ne!(log2_sub(1024, 2), log2_sub(1536, 2));
    }

    #[test]
    fn rule_precedence() {
        const fn a(name: &'static str, tag: &'static str) -> ArgSpec {
            ArgSpec::new(name, tag)
        }
        let p = KeyPolicy::parse("@size:log2; @scale:zeroone; n:exact; resize.w:drop; img_*.@size:sig/3; *:drop", KeyPolicy::exact()).unwrap();
        assert_eq!(p.bucket_for("resize", &a("n", "size")), Bucket::Exact);
        assert_eq!(p.bucket_for("resize", &a("w", "size")), Bucket::Drop);
        assert_eq!(p.bucket_for("crop", &a("w", "size")), Bucket::Log2);
        assert_eq!(p.bucket_for("img_blur", &a("w", "size")), Bucket::Sig(3));
        assert_eq!(p.bucket_for("x", &a("alpha", "scale")), Bucket::ZeroOne);
        assert_eq!(p.bucket_for("x", &a("mode", "")), Bucket::Drop);
        assert_eq!(KeyPolicy::exact().bucket_for("x", &a("mode", "")), Bucket::Exact);
        let untagged = KeyPolicy::parse("@:drop", KeyPolicy::exact()).unwrap();
        assert_eq!(untagged.bucket_for("x", &a("mode", "")), Bucket::Drop);
        assert_eq!(untagged.bucket_for("x", &a("n", "size")), Bucket::Exact);
        assert!(KeyPolicy::parse("size:sideways", KeyPolicy::exact()).is_err());
    }

    #[test]
    fn strings_are_exact_and_escaped() {
        let b = Bucket::Log2;
        let (k1, x1) = b.apply_kind(Value::Str(b"/tmp/a b,c")).unwrap();
        let (_, x2) = b.apply_kind(Value::Str(b"/tmp/a b,c")).unwrap();
        let (_, x3) = b.apply_kind(Value::Str(b"/tmp/other")).unwrap();
        assert_eq!((k1, x1), (Kind::Str, x2));
        assert_ne!(x1, x3);
        let mut s = String::new();
        b.describe(k1, x1, &mut s);
        assert_eq!(s, "/tmp/a%20b%2Cc");
        assert_eq!(b.apply(Value::Ptr(0x1000)), None);
    }
}
