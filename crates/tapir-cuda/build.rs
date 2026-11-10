//! Generate interposers from the installed CUDA math-library headers, so the
//! wrapped API always matches the CUDA version you build against. Parameters
//! are tagged with the `linalg` vocabulary from tapir-vocab, as in tapir-rocm.
//! The CUDA runtime's own calls are wrapped by hand in `src/cudart.rs` and
//! need no headers.
//!
//! Build-time environment:
//! - `CUDA_HOME` (or `CUDA_PATH`, `CUDA_ROOT`, `CUDATOOLKIT_HOME`, `NVHPC_CUDA_HOME`):
//!   CUDA root, default `/usr/local/cuda`.
//! - `TAPIR_CUDA_INCLUDE_DIRS`: `:`-separated include directories searched
//!   first (an NVIDIA HPC SDK keeps the math libraries under
//!   `math_libs/<version>/include`, next to `cuda/<version>`; that layout is
//!   found without this).
//! - `TAPIR_CUDA_LIBS`: comma list from cublas,cusolver,cusparse (default: all
//!   of them that have headers installed), or `none` for the runtime only.
//! - `TAPIR_CUDA_EXCLUDE` / `TAPIR_CUDA_INCLUDE`: extra `;`-separated name globs.

use std::path::{Path, PathBuf};

use tapir_gen::{Generator, TypeMap};
use tapir_vocab::LINALG;

/// Setup, query and bookkeeping APIs: not work, and noise in a profile.
/// The same families tapir-rocm leaves out.
const EXCLUDE: &[&str] = &[
    "*Create*", "*Destroy*", "*Set*", "*Get*", "*Version*", "*bufferSize*", "*BufferSize*",
    "*Workspace*", "*Logger*", "*Xerbla*", "*Init", "*Finalize", "*StatusToString*",
];

/// CUDA-specific parameter names the shared vocabulary does not have.
const ARGS: &str = "
batchsize           @count
group_count         @count
maxite              @count
niters              @count
nnz*                @dim
mb                  @dim
kb                  @dim
rank                @dim
blocksize           @dim
*blockdim*          @dim
lwork_bytes         @dim
workspaceinbytes*   @dim
*buffersize         @dim
reorder             @flag
econ                @flag
*_bias              @scalar
*_mult              @scalar
*_shift             @scalar
";

fn env(var: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={var}");
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

fn cuda_root() -> PathBuf {
    for var in ["CUDA_HOME", "CUDA_PATH", "CUDA_ROOT", "CUDATOOLKIT_HOME", "NVHPC_CUDA_HOME"] {
        if let Some(p) = env(var) {
            return p.into();
        }
    }
    "/usr/local/cuda".into()
}

/// Where headers may be, most specific first.
fn include_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env("TAPIR_CUDA_INCLUDE_DIRS").map(|v| v.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
    dirs.push(root.join("include"));
    dirs.push(root.join("targets/x86_64-linux/include"));
    // NVIDIA HPC SDK: <sdk>/cuda/<ver> next to <sdk>/math_libs/<ver>. Follow
    // symlinks first, since CUDA_HOME is often a link into the SDK.
    let real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let (Some(ver), Some(sdk)) = (real.file_name(), real.parent().and_then(Path::parent)) {
        dirs.push(sdk.join("math_libs").join(ver).join("include"));
        dirs.push(sdk.join("math_libs/include"));
    }
    dirs
}

fn main() {
    let root = cuda_root();
    let dirs = include_dirs(&root);
    let find = |h: &str| dirs.iter().map(|d| d.join(h)).find(|p| p.exists());
    let wanted = env("TAPIR_CUDA_LIBS").unwrap_or_else(|| "cublas,cusolver,cusparse".into());
    let wants = |lib: &str| wanted.split(',').any(|w| w.trim() == lib);

    let mut g = Generator::new(TypeMap::c_basic());
    g.rules.parse_text(LINALG.args).unwrap_or_else(|e| panic!("linalg vocabulary: {e}"));
    g.rules.parse_text(ARGS).unwrap_or_else(|e| panic!("cuda rules: {e}"));
    for pat in EXCLUDE {
        g.exclude(pat);
    }
    let globs = |var| env(var).unwrap_or_default().split(';').filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>();
    for pat in globs("TAPIR_CUDA_EXCLUDE") {
        g.exclude(&pat);
    }
    for pat in globs("TAPIR_CUDA_INCLUDE") {
        g.include(&pat);
    }
    // Calling-convention macros that expand to nothing on Linux.
    g.ignore("CUBLASWINAPI");

    // (library, export macro, headers with prototypes, headers with types only)
    let libs: &[(&str, &str, &[&str], &[&str])] = &[
        ("cublas", "CUBLASAPI", &["cublas_api.h"], &["library_types.h"]),
        ("cusolver", "CUSOLVERAPI", &["cusolverDn.h", "cusolverSp.h", "cusolverRf.h"], &["library_types.h", "cusolver_common.h", "cublas_api.h", "cusparse.h"]),
        ("cusparse", "CUSPARSEAPI", &["cusparse.h"], &["library_types.h"]),
    ];
    // Types first: the libraries use each other's handles and enums.
    let mut sources = Vec::new();
    for (lib, export, headers, type_headers) in libs {
        if !wants(lib) {
            continue;
        }
        for h in type_headers.iter().chain(headers.iter()) {
            match find(h) {
                Some(path) => {
                    println!("cargo:rerun-if-changed={}", path.display());
                    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    g.types.scan(&text);
                    if headers.contains(h) {
                        sources.push((*lib, *export, text));
                    }
                }
                None if headers.contains(h) => println!("cargo:warning=tapir-cuda: {h} not found, skipping"),
                None => {}
            }
        }
    }
    for (lib, export, text) in &sources {
        g.source_marked(text, lib, &[export]);
    }
    let n = g.functions().count();
    if n == 0 && wanted.trim() != "none" {
        let searched = dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ");
        panic!("tapir-cuda: no math-library functions found (searched {searched}); set CUDA_HOME or TAPIR_CUDA_INCLUDE_DIRS, or TAPIR_CUDA_LIBS=none for the CUDA runtime only");
    }
    println!("cargo:warning=tapir-cuda: {n} math-library wrappers from {}", root.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("wrappers.rs"), g.render()).unwrap();
    std::fs::write(out.join("report.txt"), g.report()).unwrap();
}
