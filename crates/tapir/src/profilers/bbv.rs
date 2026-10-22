//! `bbv`: a basic-block vector over calls, for SimPoint-style phase
//! analysis. Each distinct *bucketed* call (see [`KeyPolicy`]) is a block;
//! each interval yields a sparse vector of block -> weight.
//!
//! Intervals are closed online by an external driver (`tapir_interval()` or
//! `tapir_vector("bbv", 1, ...)` at its own boundaries), by the profiler
//! itself with `every=calls:N` / `every=time:S`, or, with neither, once at
//! exit.
//!
//! Every block has two ids: a dense `block` (1-based, first-seen order, what
//! SimPoint `.bb` files need) and a stable 64-bit `hash` of its label, which
//! is identical across ranks and runs and is what the vector API reports.
//!
//! Options:
//! - `key=<rules>`  added to the library's default key rules (`TAPIR_KEY`);
//!   `key=exact` ignores them and keeps every distinct signature, `key=name`
//!   keys on the function name only
//! - `weight=calls|time`  per-call contribution (time = nanoseconds)
//! - `every=calls:N|time:SECONDS`  self-driven intervals
//! - `format=simpoint|csv|tsv|jsonl`  per-interval output (default simpoint)
//! - `lib=`, `fn=`, `nested=` filters (nested defaults to off, so a call made
//!   from inside another wrapped call is not counted twice)
//!
//! Output streams: `<name>` (one vector per interval) and `<name>.blocks`
//! (block, hash, function, label, per-argument detail, totals).

use std::fmt::Write as _;

use crate::config::{Filter, Opts};
use crate::event::Event;
use crate::key::{BlockTable, KeyPolicy};
use crate::output::{Format, Sink, Table};
use crate::profiler::{Ctx, Profiler};
use crate::value::FnSpec;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weight {
    Calls,
    TimeNs,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Every {
    External,
    Calls(u64),
    TimeNs(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Out {
    /// SimPoint `.bb`: `T:1:5 :4:2` per interval, dense 1-based ids.
    Simpoint,
    /// Long-form table: interval, block, hash, weight.
    Table(Format),
}

struct Block {
    hash: u64,
    label: String,
    calls: u64,
    weight: u64,
}

pub struct Bbv {
    name: String,
    filter: Filter,
    pub policy: KeyPolicy,
    weight: Weight,
    every: Every,
    out: Out,
    table: BlockTable,
    blocks: Vec<Block>,
    /// Weight per block in the open interval, and which blocks are non-zero.
    cur: Vec<u64>,
    touched: Vec<u32>,
    cur_calls: u64,
    cur_start_ns: u64,
    intervals: u64,
    scratch: Vec<i64>,
}

/// FNV-1a 64-bit.
pub fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

impl Bbv {
    pub fn new(policy: KeyPolicy) -> Self {
        Bbv {
            name: "bbv".into(),
            filter: Filter::default(),
            policy,
            weight: Weight::Calls,
            every: Every::External,
            out: Out::Simpoint,
            table: BlockTable::default(),
            blocks: Vec::new(),
            cur: Vec::new(),
            touched: Vec::new(),
            cur_calls: 0,
            cur_start_ns: 0,
            intervals: 0,
            scratch: Vec::new(),
        }
    }

    pub fn from_opts(o: &Opts) -> Result<Bbv, String> {
        let lib_default = KeyPolicy::parse(&o.default_key, KeyPolicy::exact())?;
        let policy = match o.get("key") {
            Some("exact") => KeyPolicy::exact(),
            Some("name") => KeyPolicy::name_only(),
            Some(spec) => KeyPolicy::parse(spec, lib_default).map_err(|e| format!("{}: {e}", o.name))?,
            None => lib_default,
        };
        let mut b = Bbv::new(policy);
        b.name = o.name.clone();
        b.filter = Filter::from_opts(o);
        b.weight = match o.get("weight").unwrap_or("calls") {
            "calls" | "count" => Weight::Calls,
            "time" | "ns" => Weight::TimeNs,
            w => return Err(format!("{}: unknown weight `{w}` (calls|time)", o.name)),
        };
        b.every = match o.get("every") {
            None | Some("") | Some("external") | Some("none") => Every::External,
            Some(e) => match e.split_once(':') {
                Some(("calls", n)) => Every::Calls(n.parse().map_err(|_| format!("bad every `{e}`"))?),
                Some(("time", s)) => Every::TimeNs((s.parse::<f64>().map_err(|_| format!("bad every `{e}`"))? * 1e9) as u64),
                _ => return Err(format!("{}: every must be calls:N or time:SECONDS", o.name)),
            },
        };
        b.out = match o.get("format") {
            None | Some("simpoint") | Some("bb") => Out::Simpoint,
            Some(_) => Out::Table(o.format(Format::Csv)?),
        };
        Ok(b)
    }

    /// Add `w` to the block whose key is in `self.scratch`.
    fn add_key(&mut self, spec: &'static FnSpec, w: u64) {
        let id = self.table.intern(spec, &self.scratch);
        let i = id as usize;
        if i == self.blocks.len() {
            let label = self.policy.describe(spec, &self.scratch);
            self.blocks.push(Block { hash: fnv1a(&label), label, calls: 0, weight: 0 });
            self.cur.push(0);
        }
        if self.cur[i] == 0 {
            self.touched.push(id);
        }
        self.cur[i] += w;
        self.blocks[i].calls += 1;
        self.blocks[i].weight += w;
        self.cur_calls += 1;
    }

    /// Record one call into the open interval. Public so other profilers can
    /// embed a BBV.
    pub fn add(&mut self, c: &crate::Call) {
        self.policy.key_into(c, &mut self.scratch);
        let w = match self.weight {
            Weight::Calls => 1,
            Weight::TimeNs => c.duration_ns().max(1),
        };
        self.add_key(c.spec, w);
    }

    fn sorted_touched(&self) -> Vec<u32> {
        let mut ids = self.touched.clone();
        ids.sort_unstable();
        ids
    }

    /// Render the open interval in this profiler's text format.
    pub fn render(&self, index: u64, out: &mut String) {
        let ids = self.sorted_touched();
        match self.out {
            Out::Simpoint => {
                out.push('T');
                for (n, id) in ids.iter().enumerate() {
                    let _ = write!(out, "{}{}:{}", if n == 0 { ":" } else { " :" }, id + 1, self.cur[*id as usize]);
                }
                out.push('\n');
            }
            Out::Table(f) => {
                let t = self.interval_table(index, &ids);
                for row in &t.rows {
                    f.row(&t.columns, row, out);
                }
            }
        }
    }

    fn interval_table(&self, index: u64, ids: &[u32]) -> Table {
        let mut t = Table::new(&["interval", "block", "hash", "weight"]);
        for &id in ids {
            t.push(vec![index.into(), (id + 1).into(), self.blocks[id as usize].hash.into(), self.cur[id as usize].into()]);
        }
        t
    }

    fn reset_interval(&mut self) {
        for &id in &self.touched {
            self.cur[id as usize] = 0;
        }
        self.touched.clear();
        self.cur_calls = 0;
        self.cur_start_ns = crate::sys::now_ns();
        self.intervals += 1;
    }

    fn emit(&mut self, out: &mut Sink) {
        let index = self.intervals;
        let res = match self.out {
            Out::Table(f) => out.table(&self.name, f, &self.interval_table(index, &self.sorted_touched())),
            Out::Simpoint => {
                let mut text = String::new();
                self.render(index, &mut text);
                out.raw(&self.name, "bb").and_then(|(w, _)| w.write_all(text.as_bytes()))
            }
        };
        if let Err(e) = res {
            eprintln!("[tapir] {}: {e}", self.name);
        }
        self.reset_interval();
    }

    fn blocks_table(&self) -> Table {
        let mut t = Table::new(&["block", "hash", "lib", "function", "label", "args", "calls", "weight"]);
        for id in 0..self.blocks.len() as u32 {
            let (spec, key) = self.table.entry(id);
            let b = &self.blocks[id as usize];
            // "name@tag=bucket" (or "name=bucket") per keyed argument, for
            // offline re-bucketing.
            let mut args = String::new();
            for (a, v) in self.policy.key_args(spec, key) {
                if !args.is_empty() {
                    args.push(' ');
                }
                args.push_str(a.name);
                if !a.tag.is_empty() {
                    args.push('@');
                    args.push_str(a.tag);
                }
                args.push('=');
                args.push_str(&v);
            }
            t.push(vec![
                (id + 1).into(),
                b.hash.into(),
                spec.lib.into(),
                spec.name.into(),
                b.label.as_str().into(),
                args.into(),
                b.calls.into(),
                b.weight.into(),
            ]);
        }
        t
    }
}

impl Profiler for Bbv {
    fn name(&self) -> &str {
        &self.name
    }

    fn needs_time(&self) -> bool {
        self.weight == Weight::TimeNs || matches!(self.every, Every::TimeNs(_))
    }

    fn on_event(&mut self, ev: &Event, cx: &Ctx) {
        let t_end = match ev {
            Event::Call(c) if self.filter.accepts(c) => {
                self.add(c);
                c.t_end
            }
            _ => return,
        };
        if self.cur_start_ns == 0 {
            self.cur_start_ns = t_end;
        }
        let close = match self.every {
            Every::External => false,
            Every::Calls(n) => self.cur_calls >= n,
            Every::TimeNs(t) => t_end.saturating_sub(self.cur_start_ns) >= t,
        };
        if close {
            self.emit(&mut cx.sink());
        }
    }

    fn interval(&mut self, _index: u64, out: &mut Sink) {
        // Externally driven: emit even an empty vector, so record N of the
        // output always corresponds to interval N of the driver.
        if self.every == Every::External {
            self.emit(out);
        }
    }

    fn snapshot(&mut self, out: &mut String) -> bool {
        self.render(self.intervals, out);
        true
    }

    fn vector(&mut self, take: bool, out: &mut Vec<(u64, f64)>) -> bool {
        for id in self.sorted_touched() {
            out.push((self.blocks[id as usize].hash, self.cur[id as usize] as f64));
        }
        if take {
            self.reset_interval();
        }
        true
    }

    fn discard_interval(&mut self, _index: u64) {
        self.reset_interval();
    }

    fn finish(&mut self, out: &mut Sink) {
        if !self.touched.is_empty() || self.intervals == 0 {
            self.emit(out);
        }
        let fmt = match self.out {
            Out::Table(f) => f,
            _ => Format::Csv,
        };
        if let Err(e) = out.table(&format!("{}.blocks", self.name), fmt, &self.blocks_table()) {
            eprintln!("[tapir] {}: {e}", self.name);
        }
    }
}
