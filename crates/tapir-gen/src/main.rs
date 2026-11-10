//! `tapir-gen [options] HEADER... > wrappers.rs`
//!
//! Options (apply to the headers that follow them):
//!   --lib NAME            library tag for following headers (default: header stem)
//!   --export MACRO        only take prototypes prefixed by MACRO (repeatable);
//!                         without it every top-level prototype is taken
//!   --marker MACRO        like --export, but MACRO may stand anywhere in the
//!                         prototype (`ret MACRO name(...)`); repeatable
//!   --ignore WORD         drop WORD from prototypes (calling-convention macros)
//!   --scan-types          also register the handles, enums and scalar aliases
//!                         each following header declares with typedef
//!   --type NAME=KIND      add a type: KIND is a Rust scalar (i32, f64, ...),
//!                         `enum` or `handle` (C's basic types are built in)
//!   --arg SELECTOR=ACTION parameter rule, e.g. `n=@size`, `path=cstr@path`,
//!                         `myfn.flags=skip` (later rules win)
//!   --rules FILE          parameter rules from a file (`selector action` lines)
//!   --include GLOB        only wrap matching function names (repeatable)
//!   --exclude GLOB        never wrap matching function names (repeatable)
//!   --fortran             scalars passed by reference
//!   --report              print the wrapped/skipped report to stderr

use std::process::exit;

use tapir_gen::{Generator, TypeMap};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").strip_prefix(' ').unwrap_or("")).collect::<Vec<_>>().join("\n"));
        exit(if args.is_empty() { 2 } else { 0 });
    }
    let mut g = Generator::new(TypeMap::c_basic());
    let mut lib: Option<String> = None;
    let mut exports: Vec<String> = Vec::new();
    let mut markers: Vec<String> = Vec::new();
    let mut scan_types = false;
    let mut report = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| die(&format!("{a} needs a value")));
        match a.as_str() {
            "--lib" => lib = Some(val()),
            "--export" => exports.push(val()),
            "--marker" => markers.push(val()),
            "--ignore" => {
                g.ignore(&val());
            }
            "--scan-types" => scan_types = true,
            "--type" => {
                let v = val();
                let (n, k) = v.split_once('=').unwrap_or_else(|| die("--type wants NAME=KIND"));
                match k {
                    "enum" => g.types.enumeration(n),
                    "handle" => g.types.handle(n),
                    r => g.types.scalar(n, Box::leak(r.to_owned().into_boxed_str())),
                };
            }
            "--arg" => {
                let v = val();
                let (s, act) = v.split_once('=').unwrap_or_else(|| die("--arg wants SELECTOR=ACTION"));
                g.rules.add(s, act).unwrap_or_else(|e| die(&e));
            }
            "--rules" => {
                let path = val();
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("{path}: {e}")));
                g.rules.parse_text(&text).unwrap_or_else(|e| die(&format!("{path}: {e}")));
            }
            "--include" => {
                g.include(&val());
            }
            "--exclude" => {
                g.exclude(&val());
            }
            "--fortran" => {
                g.fortran(true);
            }
            "--report" => report = true,
            path => {
                let tag = lib.clone().unwrap_or_else(|| std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned());
                let text = std::fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{path}: {e}")));
                if scan_types {
                    g.types.scan(&text);
                }
                if markers.is_empty() {
                    let ex: Vec<&str> = exports.iter().map(String::as_str).collect();
                    g.source(&text, &tag, &ex);
                } else {
                    let mk: Vec<&str> = markers.iter().map(String::as_str).collect();
                    g.source_marked(&text, &tag, &mk);
                }
            }
        }
    }
    print!("{}", g.render());
    if report {
        eprint!("{}", g.report());
    }
}

fn die(msg: &str) -> ! {
    eprintln!("tapir-gen: {msg}");
    exit(2)
}
