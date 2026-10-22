//! `trace`: one row per event (the old "logger"), streamed in batches.
//!
//! Options: `format=csv|tsv|jsonl`, `batch=<rows>` (default 4096), plus the
//! common filter options (`lib`, `fn`, `nested`; nested defaults to on).

use std::fmt::Write as _;

use crate::config::{Filter, Opts};
use crate::event::Event;
use crate::output::{Cell, Format, Sink, Table};
use crate::profiler::{Ctx, Profiler};

pub struct Trace {
    name: String,
    filter: Filter,
    format: Format,
    batch: usize,
    rows: Table,
}

const COLUMNS: &[&str] = &["t_ns", "duration_ns", "thread", "depth", "interval", "kind", "lib", "function", "args"];

impl Trace {
    pub fn from_opts(o: &Opts) -> Result<Trace, String> {
        let mut filter = Filter::from_opts(o);
        filter.nested = o.flag("nested", true);
        Ok(Trace {
            name: o.name.clone(),
            filter,
            format: o.format(Format::Csv)?,
            batch: o.parse_or("batch", 4096)?,
            rows: Table::new(COLUMNS),
        })
    }

    fn drain(&mut self, out: &mut Sink) {
        if self.rows.rows.is_empty() {
            return;
        }
        if let Err(e) = out.table(&self.name, self.format, &self.rows) {
            eprintln!("[tapir] {}: {e}", self.name);
        }
        self.rows.rows.clear();
    }
}

impl Profiler for Trace {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_event(&mut self, ev: &Event, cx: &Ctx) {
        let iv = Cell::UInt(cx.interval);
        let row: Vec<Cell> = match ev {
            Event::Call(c) if self.filter.accepts(c) => {
                let mut args = String::new();
                for (n, _, v) in c.named_args() {
                    if v != crate::Value::None {
                        if !args.is_empty() {
                            args.push(' ');
                        }
                        let _ = write!(args, "{n}={v}");
                    }
                }
                vec![
                    c.t_start.into(),
                    c.duration_ns().into(),
                    c.thread.into(),
                    c.depth.into(),
                    iv,
                    "call".into(),
                    c.spec.lib.into(),
                    c.spec.name.into(),
                    args.into(),
                ]
            }
            Event::Marker { name, begin, t } => vec![
                (*t).into(),
                Cell::UInt(0),
                Cell::UInt(0),
                Cell::UInt(0),
                iv,
                "marker".into(),
                "".into(),
                (*name).into(),
                (if *begin { "begin" } else { "end" }).into(),
            ],
            _ => return,
        };
        self.rows.push(row);
        if self.rows.rows.len() >= self.batch {
            self.drain(&mut cx.sink());
        }
    }

    fn interval(&mut self, _index: u64, out: &mut Sink) {
        self.drain(out);
    }

    fn finish(&mut self, out: &mut Sink) {
        self.drain(out);
    }
}
