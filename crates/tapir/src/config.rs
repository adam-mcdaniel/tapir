//! Option strings: `TAPIR_PROFILERS="bbv(weight=time, lib=libA;libB), summary"`.

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::event::Call;
use crate::output::Format;

/// Options for one profiler instance.
#[derive(Clone, Debug, Default)]
pub struct Opts {
    pub name: String,
    map: BTreeMap<String, String>,
    /// Inherited defaults (`TAPIR_FORMAT`).
    pub default_format: Option<Format>,
    /// The library's default key rules plus `TAPIR_KEY` (see
    /// [`Builder::default_key`](crate::Builder::default_key)). Profilers that
    /// bucket calls start from these.
    pub default_key: String,
}

impl Opts {
    pub fn new(name: &str) -> Self {
        Opts { name: name.to_owned(), ..Default::default() }
    }

    pub fn with(mut self, k: &str, v: &str) -> Self {
        self.map.insert(k.to_owned(), v.to_owned());
        self
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.map.get(k).map(String::as_str)
    }

    pub fn parse_or<T: FromStr>(&self, k: &str, default: T) -> Result<T, String> {
        match self.get(k) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("{}: bad value `{v}` for `{k}`", self.name)),
        }
    }

    pub fn flag(&self, k: &str, default: bool) -> bool {
        match self.get(k) {
            None => default,
            Some(v) => matches!(v, "1" | "true" | "yes" | "on" | ""),
        }
    }

    pub fn format(&self, default: Format) -> Result<Format, String> {
        match self.get("format") {
            Some(f) => Format::parse(f).ok_or_else(|| format!("{}: unknown format `{f}`", self.name)),
            None => Ok(self.default_format.unwrap_or(default)),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }
}

/// Parse a profiler list: `name`, `name()`, `name(k=v, k2=v2)`, comma-separated.
pub fn parse_list(s: &str) -> Result<Vec<Opts>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let end = rest.find([',', '(']).unwrap_or(rest.len());
        let name = rest[..end].trim();
        if name.is_empty() {
            return Err(format!("expected profiler name at `{rest}`"));
        }
        let mut opts = Opts::new(name);
        rest = &rest[end..];
        if let Some(r) = rest.strip_prefix('(') {
            let close = r.find(')').ok_or_else(|| format!("unclosed `(` after `{name}`"))?;
            for kv in r[..close].split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                opts.map.insert(k.trim().to_owned(), v.trim().to_owned());
            }
            rest = &r[close + 1..];
        }
        rest = rest.trim_start().strip_prefix(',').unwrap_or(rest).trim_start();
        out.push(opts);
    }
    Ok(out)
}

/// Common call filter used by the built-in profilers:
/// `lib=libA;libB`, `fn=read*;*write*`, `nested=1`.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    libs: Vec<String>,
    fns: Vec<String>,
    /// Also accept calls made from inside other wrapped calls.
    pub nested: bool,
}

impl Filter {
    /// Accept every call, nested or not.
    pub fn all() -> Self {
        Filter { nested: true, ..Default::default() }
    }

    pub fn from_opts(o: &Opts) -> Self {
        let list = |k| o.get(k).map(|v| v.split(';').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        Filter { libs: list("lib"), fns: list("fn"), nested: o.flag("nested", false) }
    }

    pub fn accepts(&self, c: &Call) -> bool {
        (self.nested || c.depth == 0)
            && (self.libs.is_empty() || self.libs.iter().any(|l| l == c.spec.lib))
            && (self.fns.is_empty() || self.fns.iter().any(|p| glob(p, c.spec.name)))
    }
}

/// `*` and `?` wildcards.
pub fn glob(pat: &str, s: &str) -> bool {
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], s) || (!s.is_empty() && rec(p, &s[1..])),
            (Some(b'?'), Some(_)) => rec(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    rec(pat.as_bytes(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list() {
        let l = parse_list("bbv(weight=time, lib=liba;libb), summary ,trace()").unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].name, "bbv");
        assert_eq!(l[0].get("lib"), Some("liba;libb"));
        assert_eq!(l[1].name, "summary");
        assert_eq!(l[2].name, "trace");
        assert!(glob("*read*", "pread64"));
        assert!(!glob("f*", "write"));
    }
}
