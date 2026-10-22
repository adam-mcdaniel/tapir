//! Output: profilers produce [`Table`]s (or raw text), a [`Format`] renders
//! them, and the [`Sink`] decides where the bytes go.
//!
//! File names come from a template (`TAPIR_OUT`), default
//! `tapir-out/{stream}.{tag}.{ext}` where `tag` is `r<rank>` under MPI and
//! `p<pid>` otherwise. Existing files are never overwritten: a numeric suffix
//! is added instead.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
}

impl From<i64> for Cell {
    fn from(v: i64) -> Self {
        Cell::Int(v)
    }
}
impl From<u64> for Cell {
    fn from(v: u64) -> Self {
        Cell::UInt(v)
    }
}
impl From<u32> for Cell {
    fn from(v: u32) -> Self {
        Cell::UInt(v as u64)
    }
}
impl From<usize> for Cell {
    fn from(v: usize) -> Self {
        Cell::UInt(v as u64)
    }
}
impl From<f64> for Cell {
    fn from(v: f64) -> Self {
        Cell::Float(v)
    }
}
impl From<String> for Cell {
    fn from(v: String) -> Self {
        Cell::Str(v)
    }
}
impl From<&str> for Cell {
    fn from(v: &str) -> Self {
        Cell::Str(v.to_owned())
    }
}

/// A batch of rows with a fixed schema.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(columns: &[&'static str]) -> Self {
        Table { columns: columns.to_vec(), rows: Vec::new() }
    }

    pub fn push(&mut self, row: Vec<Cell>) {
        debug_assert_eq!(row.len(), self.columns.len());
        self.rows.push(row);
    }
}

/// Row-oriented text formats. Profilers with a domain-specific format (e.g.
/// SimPoint `.bb`) write raw text via [`Sink::raw`] instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    Tsv,
    /// One JSON object per row.
    Jsonl,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        Some(match s.trim() {
            "csv" => Format::Csv,
            "tsv" => Format::Tsv,
            "jsonl" | "json" | "ndjson" => Format::Jsonl,
            _ => return None,
        })
    }

    pub fn ext(&self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Jsonl => "jsonl",
        }
    }

    pub fn header(&self, cols: &[&str], out: &mut String) {
        let sep = match self {
            Format::Csv => ",",
            Format::Tsv => "\t",
            Format::Jsonl => return,
        };
        out.push_str(&cols.join(sep));
        out.push('\n');
    }

    pub fn row(&self, cols: &[&str], row: &[Cell], out: &mut String) {
        match self {
            Format::Csv | Format::Tsv => {
                let sep = if *self == Format::Csv { ',' } else { '\t' };
                for (i, c) in row.iter().enumerate() {
                    if i > 0 {
                        out.push(sep);
                    }
                    match c {
                        Cell::Str(s) if *self == Format::Csv && s.contains([',', '"', '\n']) => {
                            out.push('"');
                            out.push_str(&s.replace('"', "\"\""));
                            out.push('"');
                        }
                        Cell::Str(s) => out.push_str(s),
                        other => write_scalar(other, out),
                    }
                }
            }
            Format::Jsonl => {
                out.push('{');
                for (i, (k, c)) in cols.iter().zip(row).enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    json_str(k, out);
                    out.push(':');
                    match c {
                        Cell::Str(s) => json_str(s, out),
                        Cell::Float(f) if !f.is_finite() => out.push_str("null"),
                        other => write_scalar(other, out),
                    }
                }
                out.push('}');
            }
        }
        out.push('\n');
    }
}

fn write_scalar(c: &Cell, out: &mut String) {
    let _ = match c {
        Cell::Int(v) => write!(out, "{v}"),
        Cell::UInt(v) => write!(out, "{v}"),
        Cell::Float(v) => write!(out, "{v}"),
        Cell::Str(s) => write!(out, "{s}"),
    };
}

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Process identity used in output names.
#[derive(Clone, Debug)]
pub struct Identity {
    pub rank: Option<u32>,
    pub pid: u32,
    pub exe: String,
}

impl Identity {
    pub fn detect() -> Self {
        const RANK_VARS: &[&str] = &[
            "TAPIR_RANK",
            "OMPI_COMM_WORLD_RANK",
            "PMI_RANK",
            "PMIX_RANK",
            "SLURM_PROCID",
            "MV2_COMM_WORLD_RANK",
            "ALPS_APP_PE",
        ];
        let rank = RANK_VARS.iter().find_map(|v| std::env::var(v).ok()?.trim().parse().ok());
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "app".into());
        Identity { rank, pid: std::process::id(), exe }
    }

    pub fn tag(&self) -> String {
        match self.rank {
            Some(r) => format!("r{r}"),
            None => format!("p{}", self.pid),
        }
    }
}

/// Owns output files. Each *stream* (e.g. `"bbv"`, `"summary"`) maps to one
/// file that is created on first write and appended to afterwards.
pub struct Sink {
    template: String,
    id: Identity,
    files: HashMap<String, BufWriter<File>>,
    /// Paths created so far, for reporting at exit.
    pub created: Vec<PathBuf>,
}

impl Sink {
    pub const DEFAULT_TEMPLATE: &'static str = "tapir-out/{stream}.{tag}.{ext}";

    pub fn new(template: Option<String>, id: Identity) -> Self {
        Sink { template: template.unwrap_or_else(|| Self::DEFAULT_TEMPLATE.into()), id, files: HashMap::new(), created: Vec::new() }
    }

    pub fn identity(&self) -> &Identity {
        &self.id
    }

    fn path_for(&self, stream: &str, ext: &str) -> PathBuf {
        let rank = self.id.rank.map(|r| r.to_string()).unwrap_or_else(|| "x".into());
        PathBuf::from(
            self.template
                .replace("{stream}", stream)
                .replace("{ext}", ext)
                .replace("{tag}", &self.id.tag())
                .replace("{rank}", &rank)
                .replace("{pid}", &self.id.pid.to_string())
                .replace("{exe}", &self.id.exe),
        )
    }

    /// Writer for `stream`, opening `<template>` with extension `ext` the
    /// first time. Returns whether the file was freshly created.
    pub fn raw(&mut self, stream: &str, ext: &str) -> io::Result<(&mut dyn Write, bool)> {
        let key = format!("{stream}.{ext}");
        let fresh = !self.files.contains_key(&key);
        if fresh {
            let path = unique_path(&self.path_for(stream, ext));
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                fs::create_dir_all(dir)?;
            }
            let f = OpenOptions::new().write(true).create_new(true).open(&path)?;
            self.created.push(path);
            self.files.insert(key.clone(), BufWriter::with_capacity(1 << 16, f));
        }
        Ok((self.files.get_mut(&key).unwrap() as &mut dyn Write, fresh))
    }

    /// Append `table` to `stream` in `format`, writing the header once.
    pub fn table(&mut self, stream: &str, format: Format, table: &Table) -> io::Result<()> {
        let mut buf = String::new();
        let (w, fresh) = self.raw(stream, format.ext())?;
        if fresh {
            format.header(&table.columns, &mut buf);
        }
        for row in &table.rows {
            format.row(&table.columns, row, &mut buf);
        }
        w.write_all(buf.as_bytes())
    }

    pub fn flush(&mut self) {
        for f in self.files.values_mut() {
            let _ = f.flush();
        }
    }
}

fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|n| p.with_file_name(format!("{stem}.{n}{ext}")))
        .find(|c| !c.exists())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let mut t = Table::new(&["a", "b"]);
        t.push(vec![Cell::Int(1), "x,y".into()]);
        let mut s = String::new();
        Format::Csv.header(&t.columns, &mut s);
        Format::Csv.row(&t.columns, &t.rows[0], &mut s);
        assert_eq!(s, "a,b\n1,\"x,y\"\n");
        s.clear();
        Format::Jsonl.row(&t.columns, &t.rows[0], &mut s);
        assert_eq!(s, "{\"a\":1,\"b\":\"x,y\"}\n");
    }
}
