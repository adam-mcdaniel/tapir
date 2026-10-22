//! Generate [`tapir::interpose!`] declarations from C headers.
//!
//! Usage from a `build.rs`:
//!
//! ```ignore
//! let mut types = tapir_gen::TypeMap::c_basic();
//! types.handle("mylib_handle_t").enumeration("mylib_mode_t").scalar("mylib_int", "i32");
//! let mut g = tapir_gen::Generator::new(types);
//! g.rules.parse_text("n @size\n path cstr@path\n")?;      // optional
//! g.header("/usr/include/mylib.h", "mylib", &["MYLIB_API"])?;
//! std::fs::write(out_dir.join("wrappers.rs"), g.render())?;
//! ```
//!
//! The parser is deliberately simple: it strips comments and preprocessor
//! lines, then takes every `EXPORT_MACRO <ret> <name>(<params>);` statement
//! (or every prototype if no macro is given). Each parameter's C type is
//! mapped to an FFI type via a [`TypeMap`]. How it is captured, and its tag,
//! come from [`Rules`]; without a rule, by-value scalars are captured with
//! `val` (C `char` with `chr`), and pointers and handles are skipped.
//! Functions with a parameter type the map does not know are skipped and
//! reported, never guessed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// How a C type maps to FFI.
#[derive(Clone, Debug, PartialEq)]
pub enum CType {
    /// A by-value scalar with the given Rust FFI type (`i32`, `i64`, `f64`, ...).
    Scalar(&'static str),
    /// C `char` (`c_char`): captured as a character by default.
    Char,
    /// A by-value C enum (ABI = `int`).
    Enum,
    /// An opaque handle typedef'd to a pointer.
    Handle,
}

/// C type name -> FFI mapping. Pointers are `*const c_void`/`*mut c_void`
/// unless a capture mode needs the pointee type, so mostly by-value types
/// need entries.
#[derive(Clone, Debug, Default)]
pub struct TypeMap {
    types: BTreeMap<String, CType>,
}

impl TypeMap {
    pub fn c_basic() -> Self {
        let mut m = TypeMap::default();
        for (c, r) in [
            ("int", "i32"),
            ("signed", "i32"),
            ("unsigned", "u32"),
            ("unsigned int", "u32"),
            ("long", "i64"),
            ("long int", "i64"),
            ("unsigned long", "u64"),
            ("long long", "i64"),
            ("long long int", "i64"),
            ("unsigned long long", "u64"),
            ("unsigned long long int", "u64"),
            ("signed char", "i8"),
            ("unsigned char", "u8"),
            ("bool", "u8"),
            ("_Bool", "u8"),
            ("short", "i16"),
            ("unsigned short", "u16"),
            ("int8_t", "i8"),
            ("int16_t", "i16"),
            ("int32_t", "i32"),
            ("int64_t", "i64"),
            ("uint8_t", "u8"),
            ("uint16_t", "u16"),
            ("uint32_t", "u32"),
            ("uint64_t", "u64"),
            ("size_t", "usize"),
            ("ssize_t", "isize"),
            ("ptrdiff_t", "isize"),
            ("float", "f32"),
            ("double", "f64"),
        ] {
            m.types.insert(c.into(), CType::Scalar(r));
        }
        m.types.insert("char".into(), CType::Char);
        m
    }

    pub fn scalar(&mut self, c: &str, rust: &'static str) -> &mut Self {
        self.types.insert(c.into(), CType::Scalar(rust));
        self
    }

    pub fn enumeration(&mut self, c: &str) -> &mut Self {
        self.types.insert(c.into(), CType::Enum);
        self
    }

    pub fn handle(&mut self, c: &str) -> &mut Self {
        self.types.insert(c.into(), CType::Handle);
        self
    }

    fn get(&self, c: &str) -> Option<&CType> {
        self.types.get(c)
    }
}

/// Capture modes understood by `tapir::interpose!`.
pub const MODES: &[&str] = &["val", "deref", "chr", "deref_chr", "cstr", "sym", "skip"];

/// Kinds a rule can select instead of a parameter name.
pub const KINDS: &[&str] = &["int", "float", "char", "enum", "handle", "ptr"];

/// One parameter rule: `selector action`.
#[derive(Clone, Debug)]
struct Rule {
    /// Function-name glob, or any function.
    func: Option<String>,
    /// Parameter-name glob (lower case), or `:kind`.
    param: String,
    mode: Option<String>,
    tag: String,
}

/// Parameter rules: how to capture parameters and how to tag them.
///
/// A rule is `selector action`:
/// - selector: `param` or `func.param`, `*` globs, matched case-insensitively;
///   or a C kind instead of a name: `:int`, `:float`, `:char`, `:enum`,
///   `:handle`, `:ptr` (also `func.:kind`);
/// - action: `[mode][@tag]`, e.g. `@size`, `cstr@path`, `deref`, `skip`.
///
/// A rule naming the parameter beats a kind rule; otherwise later rules take
/// priority over earlier ones, so specific rules added after a shared
/// vocabulary override it. There are no built-in rules.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    /// Add one rule (see the type docs).
    pub fn add(&mut self, selector: &str, action: &str) -> Result<&mut Self, String> {
        let sel = selector.trim().to_ascii_lowercase();
        let (func, param) = match sel.split_once('.') {
            Some((f, p)) => (Some(f.to_owned()), p.to_owned()),
            None => (None, sel),
        };
        let action = action.trim();
        let (mode, tag) = action.split_once('@').unwrap_or((action, ""));
        let mode = (!mode.is_empty()).then(|| mode.to_owned());
        if let Some(m) = &mode {
            if !MODES.contains(&m.as_str()) {
                return Err(format!("unknown capture mode `{m}` in `{selector} {action}` (known: {})", MODES.join(", ")));
            }
        }
        if let Some(k) = param.strip_prefix(':') {
            if !KINDS.contains(&k) {
                return Err(format!("unknown kind `:{k}` in `{selector}` (known: :{})", KINDS.join(", :")));
            }
        }
        if param.is_empty() || (mode.is_none() && tag.is_empty()) {
            return Err(format!("rule `{selector} {action}` needs a parameter and a mode or @tag"));
        }
        self.rules.push(Rule { func, param, mode, tag: tag.to_owned() });
        Ok(self)
    }

    /// Add rules from text: one `selector action` per line, `#` comments.
    pub fn parse_text(&mut self, text: &str) -> Result<&mut Self, String> {
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (sel, action) = line.split_once(char::is_whitespace).ok_or_else(|| format!("line {}: `{line}` needs `selector action`", n + 1))?;
            self.add(sel, action).map_err(|e| format!("line {}: {e}", n + 1))?;
        }
        Ok(self)
    }

    fn find(&self, func: &str, param: &str, kind: &str) -> Option<&Rule> {
        let (func, param) = (func.to_ascii_lowercase(), param.to_ascii_lowercase());
        let in_func = |r: &&Rule| r.func.as_deref().is_none_or(|f| glob(f, &func));
        let by_name = self.rules.iter().rev().filter(in_func).find(|r| !r.param.starts_with(':') && glob(&r.param, &param));
        by_name.or_else(|| self.rules.iter().rev().filter(in_func).find(|r| r.param.strip_prefix(':') == Some(kind)))
    }
}

fn glob(pat: &str, s: &str) -> bool {
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], s) || (!s.is_empty() && rec(p, &s[1..])),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    rec(pat.as_bytes(), s.as_bytes())
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ffi: String,
    pub mode: String,
    pub tag: String,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub lib: String,
    pub name: String,
    pub ret: Option<String>,
    pub params: Vec<Param>,
}

/// A C declarator's type, reduced to what lowering needs.
struct Decl {
    base: String,
    /// Pointer depth, counting a trailing `[]` as one.
    ptrs: usize,
    mutable: bool,
}

impl Decl {
    fn parse(c: &str) -> Decl {
        let is_array = c.contains('[');
        let ptrs = c.matches('*').count() + is_array as usize;
        let base = c
            .replace(['*', '&'], " ")
            .split(['[', ']'])
            .next()
            .unwrap_or("")
            .split_whitespace()
            .filter(|w| !matches!(*w, "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "struct" | "enum"))
            .collect::<Vec<_>>()
            .join(" ");
        let mutable = !c.trim_start().starts_with("const") || ptrs > 1;
        Decl { base, ptrs, mutable }
    }
}

pub struct Generator {
    pub types: TypeMap,
    pub rules: Rules,
    funcs: BTreeMap<String, Func>,
    /// Name globs to exclude.
    exclude: Vec<String>,
    include: Vec<String>,
    /// By-reference calling convention (Fortran): read tagged scalar
    /// pointers with `deref` and `char` pointers with `deref_chr`.
    by_ref: bool,
    /// (function, reason) for everything that was not wrapped.
    pub skipped: Vec<(String, String)>,
}

/// Rust keywords that appear as C parameter names.
const KEYWORDS: &[&str] = &["type", "fn", "ref", "mod", "in", "loop", "match", "move", "self", "use", "where", "impl", "box", "dyn", "struct", "trait", "async", "await", "crate", "super", "let", "mut", "pub", "static", "const", "return"];

impl Generator {
    pub fn new(types: TypeMap) -> Self {
        Generator { types, rules: Rules::default(), funcs: BTreeMap::new(), exclude: Vec::new(), include: Vec::new(), by_ref: false, skipped: Vec::new() }
    }

    /// By-reference calling convention (Fortran): scalars are passed by host
    /// pointer, so parameters with a tag are read through the pointer, and
    /// `char` pointers as one character.
    pub fn fortran(&mut self, on: bool) -> &mut Self {
        self.by_ref = on;
        self
    }

    /// Skip functions whose name matches `pattern` (`*` globs).
    pub fn exclude(&mut self, pattern: &str) -> &mut Self {
        self.exclude.push(pattern.into());
        self
    }

    /// If any include patterns are given, only matching functions are wrapped.
    pub fn include(&mut self, pattern: &str) -> &mut Self {
        self.include.push(pattern.into());
        self
    }

    /// Parse a header file; see [`Generator::source`].
    pub fn header(&mut self, path: impl AsRef<Path>, lib: &str, export_macros: &[&str]) -> std::io::Result<usize> {
        let text = std::fs::read_to_string(path)?;
        Ok(self.source(&text, lib, export_macros))
    }

    /// Parse C source text. If `export_macros` is empty, every top-level
    /// prototype (`ret name(params);`) is taken, which suits plain
    /// prototype lists. Returns the number of functions added.
    pub fn source(&mut self, text: &str, lib: &str, export_macros: &[&str]) -> usize {
        let clean = strip_comments_and_cpp(text);
        let mut added = 0;
        for stmt in clean.split([';', '{', '}']) {
            let stmt = stmt.split_whitespace().collect::<Vec<_>>().join(" ");
            let body = if export_macros.is_empty() {
                stmt.as_str()
            } else {
                match export_macros.iter().find_map(|m| stmt.find(&format!("{m} ")).map(|i| &stmt[i + m.len() + 1..])) {
                    Some(b) => b,
                    None => continue,
                }
            };
            if let Some(f) = self.parse_proto(body, lib) {
                if !self.funcs.contains_key(&f.name) {
                    added += 1;
                    self.funcs.insert(f.name.clone(), f);
                }
            }
        }
        added
    }

    fn parse_proto(&mut self, s: &str, lib: &str) -> Option<Func> {
        let open = s.find('(')?;
        let mut depth = 0;
        let close = open + s[open..].find(|c| {
            depth += match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            depth == 0
        })?;
        let trailing = s[close + 1..].trim();
        if !(trailing.is_empty() || trailing.starts_with("__attribute__") || trailing.starts_with("noexcept")) {
            return None;
        }
        let head = s[..open].trim();
        let name_start = head.rfind(|c: char| !(c.is_alphanumeric() || c == '_'))?.wrapping_add(1);
        let name = &head[name_start..];
        let ret_c = head[..name_start].trim();
        if name.is_empty() || ret_c.is_empty() || ret_c.starts_with("typedef") || ret_c.contains("static") || ret_c.contains("inline") {
            return None;
        }
        let ret_c = strip_attributes(ret_c);
        if (!self.include.is_empty() && !self.include.iter().any(|p| glob(p, name))) || self.exclude.iter().any(|p| glob(p, name)) {
            return None;
        }
        let ret = match self.ret_type(&ret_c) {
            Ok(r) => r,
            Err(e) => {
                self.skipped.push((name.into(), format!("return type: {e}")));
                return None;
            }
        };
        let mut params = Vec::new();
        let plist = s[open + 1..close].trim();
        if !(plist.is_empty() || plist == "void") {
            for (i, p) in split_params(plist).iter().enumerate() {
                if p.contains("...") {
                    self.skipped.push((name.into(), "variadic".into()));
                    return None;
                }
                let (cty, pname) = split_decl(p, i);
                match self.lower(name, &pname, &cty) {
                    Ok(Some((ffi, mode, tag))) => {
                        let pname = if KEYWORDS.contains(&pname.as_str()) { format!("{pname}_") } else { pname };
                        params.push(Param { name: pname, ffi, mode, tag });
                    }
                    Ok(None) => {
                        self.skipped.push((name.into(), format!("void parameter `{p}`")));
                        return None;
                    }
                    Err(e) => {
                        self.skipped.push((name.into(), format!("parameter `{p}`: {e}")));
                        return None;
                    }
                }
            }
        }
        dedup_names(&mut params);
        Some(Func { lib: lib.into(), name: name.into(), ret, params })
    }

    fn ret_type(&self, c: &str) -> Result<Option<String>, String> {
        let d = Decl::parse(c);
        if d.ptrs > 0 {
            return Ok(Some(if d.mutable { "*mut c_void" } else { "*const c_void" }.into()));
        }
        match (d.base.as_str(), self.types.get(&d.base)) {
            ("void", _) => Ok(None),
            (_, Some(CType::Scalar(r))) => Ok(Some((*r).into())),
            (_, Some(CType::Char)) => Ok(Some("c_char".into())),
            (_, Some(CType::Enum)) => Ok(Some("i32".into())),
            (_, Some(CType::Handle)) => Ok(Some("*mut c_void".into())),
            (b, None) => Err(format!("unknown by-value type `{b}`")),
        }
    }

    /// Map parameter `pname: c` of `func` to (FFI type, capture mode, tag).
    /// `Ok(None)` is `void`.
    fn lower(&self, func: &str, pname: &str, c: &str) -> Result<Option<(String, String, String)>, String> {
        let d = Decl::parse(c);
        let kind = match (d.ptrs, self.types.get(&d.base)) {
            (0, Some(CType::Scalar(r))) if r.starts_with('f') => "float",
            (0, Some(CType::Scalar(_))) => "int",
            (0, Some(CType::Char)) => "char",
            (0, Some(CType::Enum)) => "enum",
            (0, Some(CType::Handle)) => "handle",
            (0, None) => "",
            _ => "ptr",
        };
        let rule = self.rules.find(func, pname, kind);
        let tag = rule.map(|r| r.tag.clone()).unwrap_or_default();
        let forced = rule.and_then(|r| r.mode.as_deref());
        let ptr = if d.mutable { "*mut" } else { "*const" };
        if d.ptrs == 0 {
            if d.base == "void" {
                return Ok(None);
            }
            let (ffi, default, allowed): (String, &str, &[&str]) = match self.types.get(&d.base) {
                Some(CType::Scalar(r)) => ((*r).into(), "val", &["val", "skip"]),
                Some(CType::Char) => ("c_char".into(), "chr", &["chr", "val", "skip"]),
                Some(CType::Enum) => ("i32".into(), "val", &["val", "skip"]),
                Some(CType::Handle) => ("*mut c_void".into(), "skip", &["skip", "val", "sym"]),
                None => return Err(format!("unknown by-value type `{}`", d.base)),
            };
            let mode = forced.unwrap_or(default);
            if !allowed.contains(&mode) {
                return Err(format!("mode `{mode}` does not fit by-value `{}`", d.base));
            }
            return Ok(Some((ffi, mode.into(), tag)));
        }
        let pointee = if d.ptrs == 1 { self.types.get(&d.base) } else { None };
        let mode = forced.unwrap_or(match pointee {
            Some(CType::Char) if self.by_ref => "deref_chr",
            Some(CType::Scalar(_) | CType::Enum) if self.by_ref && !tag.is_empty() => "deref",
            _ => "skip",
        });
        let ffi = match (mode, pointee) {
            ("deref", Some(CType::Scalar(r))) => format!("{ptr} {r}"),
            ("deref", Some(CType::Enum)) => format!("{ptr} i32"),
            ("deref", Some(CType::Char)) => format!("{ptr} c_char"),
            ("deref_chr" | "cstr", Some(CType::Char)) => format!("{ptr} c_char"),
            ("deref_chr" | "cstr", Some(CType::Scalar(r))) if matches!(*r, "u8" | "i8") => format!("{ptr} {r}"),
            ("deref" | "deref_chr" | "cstr", _) => return Err(format!("mode `{mode}` needs a pointer to a known scalar, not `{c}`")),
            ("chr", _) => return Err(format!("mode `chr` does not fit pointer `{c}`")),
            _ => format!("{ptr} c_void"),
        };
        Ok(Some((ffi, mode.into(), tag)))
    }

    pub fn functions(&self) -> impl Iterator<Item = &Func> {
        self.funcs.values()
    }

    /// Render all collected functions as one `tapir::interpose!` block.
    pub fn render(&self) -> String {
        // A private module keeps the generated `use` from clashing with the
        // including crate; `#[no_mangle]` symbols are exported regardless.
        let mut out = String::from("// @generated by tapir-gen. Do not edit.\n#[allow(unused_imports, non_snake_case)]\nmod __tapir_generated {\nuse ::core::ffi::{c_char, c_void};\n\n::tapir::interpose! {\n");
        let mut by_lib: BTreeMap<&str, Vec<&Func>> = BTreeMap::new();
        for f in self.funcs.values() {
            by_lib.entry(&f.lib).or_default().push(f);
        }
        for (lib, funcs) in by_lib {
            let _ = writeln!(out, "    lib {lib:?};");
            for f in funcs {
                let _ = write!(out, "    fn {}(", f.name);
                for (i, p) in f.params.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{}: {} => {}", p.name, p.ffi, p.mode);
                    if !p.tag.is_empty() && p.mode != "skip" {
                        let _ = write!(out, " {:?}", p.tag);
                    }
                }
                out.push(')');
                if let Some(r) = &f.ret {
                    let _ = write!(out, " -> {r}");
                }
                out.push_str(";\n");
            }
        }
        out.push_str("}\n}\n");
        out
    }

    /// A short report of what was and was not wrapped.
    pub fn report(&self) -> String {
        let mut s = format!("tapir-gen: {} functions wrapped, {} skipped\n", self.funcs.len(), self.skipped.len());
        let reasons: BTreeSet<_> = self.skipped.iter().map(|(_, r)| r.as_str()).collect();
        for r in reasons.iter().take(20) {
            let n = self.skipped.iter().filter(|(_, x)| x == r).count();
            let _ = writeln!(s, "  {n:5} x {r}");
        }
        s
    }
}

fn strip_attributes(s: &str) -> String {
    s.split_whitespace().filter(|w| !w.starts_with("__attribute__") && !w.starts_with("[[") && !w.ends_with("]]") && !w.ends_with("_DEPRECATED") && *w != "extern").collect::<Vec<_>>().join(" ")
}

fn strip_comments_and_cpp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        if b[i..].starts_with(b"/*") {
            i = text[i + 2..].find("*/").map_or(b.len(), |j| i + 2 + j + 2);
            out.push(' ');
            continue;
        }
        if b[i..].starts_with(b"//") {
            i = text[i..].find('\n').map_or(b.len(), |j| i + j);
            continue;
        }
        if line_start && b[i] == b'#' {
            // Skip the directive including backslash continuations.
            while i < b.len() && b[i] != b'\n' {
                if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                i += 1;
            }
            continue;
        }
        if b[i] == b'"' {
            // Strings never matter for prototypes (extern "C" etc.).
            let end = text[i + 1..].find('"').map_or(b.len(), |j| i + 1 + j + 1);
            out.push_str("\"\"");
            i = end;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        if ch == '\n' {
            line_start = true;
        } else if !ch.is_whitespace() {
            line_start = false;
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn split_params(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_owned());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// Split `const double* const A[]` into (`const double* const []`, `A`).
fn split_decl(p: &str, index: usize) -> (String, String) {
    let p = p.split('=').next().unwrap_or(p).trim(); // C++ default args
    // Function pointer: `ret (*name)(params)` -> (`ret (*)(params)`, `name`).
    if let Some(i) = p.find("(*") {
        let rest = &p[i + 2..];
        let end = rest.find(')').unwrap_or(rest.len());
        let name = rest[..end].trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return (format!("{}(*){}", &p[..i], &rest[end..].strip_prefix(')').unwrap_or("")), name.to_owned());
        }
        return (p.to_owned(), format!("arg{index}"));
    }
    let (decl, array) = match p.find('[') {
        Some(i) => (&p[..i], &p[i..]),
        None => (p, ""),
    };
    let decl = decl.trim_end();
    let split = decl.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let (ty, name) = (&decl[..split], &decl[split..]);
    let is_type_word = |w: &str| matches!(w, "int" | "char" | "float" | "double" | "long" | "short" | "unsigned" | "void" | "const") || w.ends_with("_t");
    if name.is_empty() || ty.trim().is_empty() || (is_type_word(name) && !ty.trim().ends_with('*')) {
        // Unnamed parameter.
        (format!("{decl}{array}"), format!("arg{index}"))
    } else {
        (format!("{ty}{array}"), name.to_owned())
    }
}

fn dedup_names(ps: &mut [Param]) {
    let mut seen = BTreeSet::new();
    for (i, p) in ps.iter_mut().enumerate() {
        if !seen.insert(p.name.clone()) {
            p.name = format!("{}_{i}", p.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"
        /* comment */
        #define FOO(x) \
            bar
        typedef struct _img *img_t;
        IMG_API int img_resize(img_t      handle,
                               img_mode   mode,
                               int        width,
                               int        height, // trailing
                               const float* kernel,
                               float      scale,
                               char       quality);
        IMG_API int img_load(img_t h, const char* path, const unsigned char* const rows[], size_t n);
        IMG_API int img_weird(struct opaque_by_value x);
        IMG_API void img_each(img_t h, void (*fn)(int));
    "#;

    fn types() -> TypeMap {
        let mut t = TypeMap::c_basic();
        t.handle("img_t").enumeration("img_mode");
        t
    }

    #[test]
    fn defaults_capture_scalars_only() {
        let mut g = Generator::new(types());
        assert_eq!(g.source(HEADER, "img", &["IMG_API"]), 3);
        let out = g.render();
        assert!(out.contains("fn img_resize(handle: *mut c_void => skip, mode: i32 => val, width: i32 => val, height: i32 => val, kernel: *const c_void => skip, scale: f32 => val, quality: c_char => chr) -> i32;"), "{out}");
        assert!(out.contains("fn img_load(h: *mut c_void => skip, path: *const c_void => skip, rows: *mut c_void => skip, n: usize => val) -> i32;"), "{out}");
        assert_eq!(g.skipped.len(), 1, "{:?}", g.skipped);
    }

    #[test]
    fn rules_set_mode_and_tag() {
        let mut g = Generator::new(types());
        g.rules
            .parse_text("# shared vocabulary\nwidth @size\nheight @size\nscale @scale\npath cstr@path\nn @count\n")
            .unwrap();
        g.rules.add("img_resize.height", "skip").unwrap(); // later rules win
        g.rules.parse_text(":enum @mode\n:float @real\nimg_each.fn sym\n").unwrap(); // names beat kinds
        g.source(HEADER, "img", &["IMG_API"]);
        let out = g.render();
        assert!(out.contains("mode: i32 => val \"mode\", width: i32 => val \"size\", height: i32 => skip, kernel: *const c_void => skip, scale: f32 => val \"scale\""), "{out}");
        assert!(out.contains("fn img_each(h: *mut c_void => skip, fn_: *mut c_void => sym);"), "{out}");
        assert!(out.contains("path: *const c_char => cstr \"path\""), "{out}");
        assert!(out.contains("n: usize => val \"count\""), "{out}");
        assert!(Rules::default().add("x", "cstrr").is_err());
        assert!(Rules::default().add("x", "").is_err());
        assert!(Rules::default().add(":struct", "@x").is_err());
    }

    #[test]
    fn incompatible_mode_is_reported() {
        let mut g = Generator::new(types());
        g.rules.add("kernel", "cstr").unwrap();
        g.source(HEADER, "img", &["IMG_API"]);
        assert!(g.functions().all(|f| f.name != "img_resize"));
        assert!(g.skipped.iter().any(|(f, r)| f == "img_resize" && r.contains("mode `cstr`")), "{:?}", g.skipped);
    }

    #[test]
    fn plain_prototypes_and_by_ref() {
        let mut g = Generator::new(TypeMap::c_basic());
        g.source("void solve_(const char* job, const int* n, int, double tol);\nint foo(void);", "flib", &[]);
        let out = g.render();
        assert!(out.contains("fn solve_(job: *const c_void => skip, n: *const c_void => skip, arg2: i32 => val, tol: f64 => val);"), "{out}");
        assert!(out.contains("fn foo() -> i32;"), "{out}");

        let mut g = Generator::new(TypeMap::c_basic());
        g.rules.add("n", "@size").unwrap();
        g.fortran(true).source("void solve_(const char* job, const int* n, double* a, int* info, const double* tol);", "flib", &[]);
        let out = g.render();
        assert!(out.contains("fn solve_(job: *const c_char => deref_chr, n: *const i32 => deref \"size\", a: *mut c_void => skip, info: *mut c_void => skip, tol: *const c_void => skip);"), "{out}");
    }
}
