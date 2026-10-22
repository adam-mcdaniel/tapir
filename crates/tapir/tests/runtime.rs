//! Integration test of the runtime without interposition: hand-made FnSpecs
//! driven through `tapir::call`, a custom profiler, default key rules,
//! intervals, and the vector API. (One test, because the runtime is
//! process-global.)

use tapir::{ArgSpec, Ctx, Event, FnSpec, Format, Opts, Profiler, Sink, Table, Value};

/// Total pixels written by `resize` calls.
struct Pixels {
    name: String,
    total: i64,
}

impl Profiler for Pixels {
    fn name(&self) -> &str {
        &self.name
    }
    fn needs_time(&self) -> bool {
        false
    }
    fn on_event(&mut self, ev: &Event, _: &Ctx) {
        if let Event::Call(c) = ev {
            if c.spec.name == "resize" {
                self.total += c.tagged("size").filter_map(|v| v.as_i64()).product::<i64>();
            }
        }
    }
    fn finish(&mut self, out: &mut Sink) {
        let mut t = Table::new(&["pixels"]);
        t.push(vec![self.total.into()]);
        let _ = out.table(&self.name, Format::Csv, &t);
    }
}

static RESIZE: FnSpec = FnSpec::new(
    "imglib",
    "resize",
    &[ArgSpec::new("mode", ""), ArgSpec::new("w", "size"), ArgSpec::new("h", "size"), ArgSpec::new("buf", "")],
);

static LOAD: FnSpec = FnSpec::new("imglib", "load", &[ArgSpec::new("path", "path"), ArgSpec::new("flags", "")]);

fn resize(mode: u8, w: i64, h: i64) -> i32 {
    let args = [Value::Char(mode), Value::Int(w), Value::Int(h), Value::None];
    tapir::call(&RESIZE, &args, || 0)
}

fn load(path: &str) {
    let args = [Value::Str(path.as_bytes()), Value::Int(0)];
    tapir::call(&LOAD, &args, || ())
}

#[test]
fn runtime_end_to_end() {
    let dir = std::env::temp_dir().join(format!("tapir-test-{}", std::process::id()));
    let template = format!("{}/{{stream}}.{{ext}}", dir.display());
    assert!(tapir::init(|b| {
        b.factory("pixels", |o: &Opts| Ok(Pixels { name: o.name.clone(), total: 0 }));
        b.output(&template);
        b.enable(Opts::new("bbv"));
        b.enable(Opts::new("pixels"));
        // Set after `enable`: named profilers are built when setup ends.
        b.default_key("@size:log2");
    }));
    let rt = tapir::runtime().unwrap();

    // Interval 0: two sizes in the same log2 bucket, one with another mode.
    resize(b'N', 1000, 1000);
    resize(b'N', 600, 1000);
    resize(b'B', 1000, 1000);
    let v0 = rt.vector("bbv", true, 16).unwrap().unwrap();
    assert_eq!(v0.len(), 2, "{v0:?}");
    assert_eq!(v0.iter().map(|x| x.1).sum::<f64>(), 3.0);

    // Too small a buffer: nothing is taken.
    resize(b'N', 4000, 4000);
    resize(b'B', 4000, 4000);
    assert_eq!(rt.vector("bbv", true, 1).unwrap(), Err(2));
    let v1 = rt.vector("bbv", true, 16).unwrap().unwrap();
    assert_eq!(v1.len(), 2);
    assert!(v1.iter().all(|(h, _)| v0.iter().all(|(h0, _)| h0 != h)), "new sizes must be new blocks");

    // Paused: not recorded.
    rt.set_enabled(false);
    resize(b'N', 1000, 1000);
    rt.set_enabled(true);
    assert!(rt.vector("bbv", false, 16).unwrap().unwrap().is_empty());

    // Untagged and string arguments are keyed exactly.
    load("/data/a.png");
    load("/data/a.png");
    load("/data/b c.png");
    let v2 = rt.vector("bbv", false, 16).unwrap().unwrap();
    assert_eq!(v2.iter().map(|x| x.1).collect::<Vec<_>>(), vec![2.0, 1.0]);

    rt.finish();
    let pixels = std::fs::read_to_string(dir.join("pixels.csv")).unwrap();
    let total: i64 = pixels.lines().nth(1).unwrap().parse().unwrap();
    assert_eq!(total, 1000 * 1000 * 2 + 600 * 1000 + 4000 * 4000 * 2);
    let blocks = std::fs::read_to_string(dir.join("bbv.blocks.csv")).unwrap();
    assert_eq!(blocks.lines().count(), 1 + 6, "{blocks}");
    // Chars are keyed by code point: 'B' == 66.
    assert!(blocks.contains("resize(mode=66,w=512..1023,h=512..1023)"), "{blocks}");
    assert!(blocks.contains("mode=66 w@size=512..1023 h@size=512..1023"), "{blocks}");
    assert!(blocks.contains("load(path=/data/b%20c.png,flags=0)"), "{blocks}");
    std::fs::remove_dir_all(&dir).ok();
}
