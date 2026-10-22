//! `summary`: per-signature call counts and timing (the old "summarylog").
//!
//! Options: `key=exact|default|name` or rules (see [`KeyPolicy`]); `default`
//! is the library's default key rules, and rules are applied on top of exact,
//! `format=csv|tsv|jsonl`, `every=<intervals>` to also dump partial summaries
//! at interval boundaries, plus the common filter options (`lib`, `fn`, `nested`).

use crate::config::{Filter, Opts};
use crate::event::Event;
use crate::key::{BlockTable, KeyPolicy};
use crate::output::{Cell, Format, Sink, Table};
use crate::profiler::{Ctx, Profiler};

#[derive(Clone, Copy, Default)]
struct Stats {
    calls: u64,
    total_ns: u64,
    min_ns: u64,
    max_ns: u64,
    /// Calls that were made from inside another wrapped call.
    nested: u64,
}

pub struct Summary {
    name: String,
    filter: Filter,
    policy: KeyPolicy,
    format: Format,
    every: u64,
    blocks: BlockTable,
    stats: Vec<Stats>,
    scratch: Vec<i64>,
}

impl Summary {
    pub fn new(policy: KeyPolicy) -> Self {
        Summary {
            name: "summary".into(),
            filter: Filter::all(),
            policy,
            format: Format::Csv,
            every: 0,
            blocks: BlockTable::default(),
            stats: Vec::new(),
            scratch: Vec::new(),
        }
    }

    pub fn from_opts(o: &Opts) -> Result<Summary, String> {
        let policy = match o.get("key").unwrap_or("exact") {
            "exact" => KeyPolicy::exact(),
            "default" => KeyPolicy::parse(&o.default_key, KeyPolicy::exact())?,
            "name" => KeyPolicy::name_only(),
            spec => KeyPolicy::parse(spec, KeyPolicy::exact())?,
        };
        let mut s = Summary::new(policy);
        s.name = o.name.clone();
        s.filter = Filter::from_opts(o);
        s.filter.nested = o.flag("nested", true);
        s.format = o.format(Format::Csv)?;
        s.every = o.parse_or("every", 0)?;
        Ok(s)
    }

    fn table(&self) -> Table {
        let mut t = Table::new(&["lib", "function", "signature", "calls", "nested_calls", "total_s", "mean_s", "min_s", "max_s"]);
        let mut order: Vec<u32> = (0..self.blocks.len() as u32).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.stats[i as usize].total_ns));
        for id in order {
            let st = self.stats[id as usize];
            if st.calls == 0 {
                continue;
            }
            let (spec, key) = self.blocks.entry(id);
            let s = |ns: u64| Cell::Float(ns as f64 * 1e-9);
            t.push(vec![
                spec.lib.into(),
                spec.name.into(),
                self.policy.describe(spec, key).into(),
                st.calls.into(),
                st.nested.into(),
                s(st.total_ns),
                Cell::Float(st.total_ns as f64 * 1e-9 / st.calls as f64),
                s(st.min_ns),
                s(st.max_ns),
            ]);
        }
        t
    }
}

impl Profiler for Summary {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        let Event::Call(c) = ev else { return };
        if !self.filter.accepts(c) {
            return;
        }
        self.policy.key_into(c, &mut self.scratch);
        let id = self.blocks.intern(c.spec, &self.scratch) as usize;
        if id >= self.stats.len() {
            self.stats.resize(id + 1, Stats { min_ns: u64::MAX, ..Default::default() });
        }
        let st = &mut self.stats[id];
        let d = c.duration_ns();
        st.calls += 1;
        st.total_ns += d;
        st.min_ns = st.min_ns.min(d);
        st.max_ns = st.max_ns.max(d);
        st.nested += (c.depth > 0) as u64;
    }

    fn interval(&mut self, index: u64, out: &mut Sink) {
        #[allow(clippy::manual_is_multiple_of)] // keep building on Rust < 1.87
        if self.every > 0 && (index + 1) % self.every == 0 {
            let _ = out.table(&format!("{}.partial{index}", self.name), self.format, &self.table());
        }
    }

    fn finish(&mut self, out: &mut Sink) {
        if let Err(e) = out.table(&self.name.clone(), self.format, &self.table()) {
            eprintln!("[tapir] {}: {e}", self.name);
        }
    }
}
