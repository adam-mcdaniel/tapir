//! A complete tapir preload library: wrappers generated from a header, the
//! built-in profilers, and one custom profiler.

use tapir::{Cell, Ctx, Event, Format, Opts, Profiler, Sink, Table};

tapir::preload!(|b| {
    // Bucketing defaults for linear-algebra arguments, a custom profiler
    // selectable by name, then TAPIR_PROFILERS (default: "bbv,summary").
    b.default_key(tapir_vocab::LINALG.key);
    b.factory("flops", Flops::from_opts);
    b.from_env("bbv,summary");
});

include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));

/// Custom profiler: estimated GEMM FLOPs (2mnk) per interval.
///
/// `TAPIR_PROFILERS="flops(min_dim=1024)"` counts only GEMMs whose smallest
/// dimension is at least 1024 (the kind of gate an Ozaki-scheme
/// profitability study uses).
struct Flops {
    name: String,
    min_dim: i64,
    format: Format,
    cur: (u64, f64),
    rows: Table,
}

impl Flops {
    fn from_opts(o: &Opts) -> Result<Flops, String> {
        Ok(Flops {
            name: o.name.clone(),
            min_dim: o.parse_or("min_dim", 0)?,
            format: o.format(Format::Csv)?,
            cur: (0, 0.0),
            rows: Table::new(&["interval", "gemm_calls", "gflop"]),
        })
    }
}

impl Profiler for Flops {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        let Event::Call(c) = ev else { return };
        if c.depth > 0 || !c.spec.name.ends_with("gemm") {
            return;
        }
        let dim = |n| c.arg(n).and_then(|v| v.as_i64()).unwrap_or(0);
        let (m, n, k) = (dim("m"), dim("n"), dim("k"));
        if m.min(n).min(k) >= self.min_dim {
            self.cur.0 += 1;
            self.cur.1 += 2.0 * m as f64 * n as f64 * k as f64;
        }
    }

    fn interval(&mut self, index: u64, _: &mut Sink) {
        self.rows.push(vec![index.into(), self.cur.0.into(), Cell::Float(self.cur.1 * 1e-9)]);
        self.cur = (0, 0.0);
    }

    fn finish(&mut self, out: &mut Sink) {
        if self.cur.0 > 0 || self.rows.rows.is_empty() {
            let idx = self.rows.rows.len() as u64;
            self.interval(idx, out);
        }
        let _ = out.table(&self.name, self.format, &self.rows);
    }
}
