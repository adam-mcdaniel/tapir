//! Generate interposers from the installed ROCm headers, so the wrapped API
//! always matches the ROCm version you build against. Parameters are tagged
//! with the `linalg` vocabulary from tapir-vocab.
//!
//! Build-time environment:
//! - `ROCM_PATH` (or `ROCM_HOME`, `CRAY_ROCM_DIR`, `HIP_PATH/..`): ROCm root,
//!   default `/opt/rocm`.
//! - `TAPIR_ROCM_LIBS`: comma list from rocblas,rocsolver,hipblas,hipsolver,rocsparse
//!   (default: all of them that have headers installed).
//! - `TAPIR_ROCM_EXCLUDE` / `TAPIR_ROCM_INCLUDE`: extra `;`-separated name globs.

use std::path::{Path, PathBuf};

use tapir_gen::{Generator, TypeMap};
use tapir_vocab::LINALG;

/// Setup, query and bookkeeping APIs: not work, and noise in a profile.
const EXCLUDE: &[&str] = &[
    "*_create*", "*_destroy*", "*_set_*", "*_get_*", "*_query*", "*_version*", "*_initialize",
    "*_is_*", "*_abort", "*status_to_string", "*buffer_size*", "*_start_*", "*_stop_*",
    "*_log_*", "*device_malloc*", "*_info_*", "*_extract_*",
    "*Create*", "*Destroy*", "*Set*", "*Get*", "*Version*", "*bufferSize*", "*BufferSize*",
    "*StatusToString*", "*Workspace*",
];

fn rocm_root() -> PathBuf {
    for var in ["ROCM_PATH", "ROCM_HOME", "CRAY_ROCM_DIR"] {
        println!("cargo:rerun-if-env-changed={var}");
        if let Some(p) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return p.into();
        }
    }
    println!("cargo:rerun-if-env-changed=HIP_PATH");
    if let Some(p) = std::env::var_os("HIP_PATH") {
        return Path::new(&p).join("..");
    }
    for p in ["/opt/rocm", "/opt/rocm-default"] {
        if Path::new(p).join("include").exists() {
            return p.into();
        }
    }
    // Newest versioned install, e.g. /opt/rocm-7.0.2.
    let mut versions: Vec<(Vec<u32>, PathBuf)> = std::fs::read_dir("/opt")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let v = name.strip_prefix("rocm-")?.split('.').map(|x| x.parse().ok()).collect::<Option<Vec<u32>>>()?;
            Some((v, e.path()))
        })
        .collect();
    versions.sort();
    versions.pop().map(|(_, p)| p).unwrap_or_else(|| "/opt/rocm".into())
}

fn main() {
    let root = rocm_root();
    let inc = root.join("include");
    println!("cargo:warning=tapir-rocm: generating wrappers from {}", root.display());
    println!("cargo:rerun-if-env-changed=TAPIR_ROCM_LIBS");
    println!("cargo:rerun-if-env-changed=TAPIR_ROCM_EXCLUDE");
    println!("cargo:rerun-if-env-changed=TAPIR_ROCM_INCLUDE");
    let wanted = std::env::var("TAPIR_ROCM_LIBS").unwrap_or_else(|_| "rocblas,rocsolver,hipblas,hipsolver,rocsparse".into());

    let mut g = Generator::new(rocm_types());
    g.rules.parse_text(LINALG.args).unwrap_or_else(|e| panic!("linalg vocabulary: {e}"));
    for pat in EXCLUDE {
        g.exclude(pat);
    }
    let globs = |var| std::env::var(var).unwrap_or_default().split(';').filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>();
    for pat in globs("TAPIR_ROCM_EXCLUDE") {
        g.exclude(&pat);
    }
    for pat in globs("TAPIR_ROCM_INCLUDE") {
        g.include(&pat);
    }

    let libs: &[(&str, &str, &[&str])] = &[
        ("rocblas", "ROCBLAS_EXPORT", &["rocblas/internal/rocblas-functions.h", "rocblas/internal/rocblas-beta.h"]),
        ("rocsolver", "ROCSOLVER_EXPORT", &["rocsolver/rocsolver-functions.h"]),
        ("hipblas", "HIPBLAS_EXPORT", &["hipblas/hipblas.h"]),
        (
            "hipsolver",
            "HIPSOLVER_EXPORT",
            &[
                "hipsolver/internal/hipsolver-functions.h",
                "hipsolver/internal/hipsolver-compat.h",
                "hipsolver/internal/hipsolver-dense.h",
                "hipsolver/internal/hipsolver-dense64.h",
            ],
        ),
    ];
    for (lib, export, headers) in libs {
        if !wanted.split(',').any(|w| w.trim() == *lib) {
            continue;
        }
        for h in *headers {
            let path = inc.join(h);
            if !path.exists() {
                println!("cargo:warning=tapir-rocm: {} not found, skipping", path.display());
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            g.header(&path, lib, &[export]).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
    // rocSPARSE: one header per routine under rocsparse/internal/, and its
    // handle and enum types come from rocsparse-types.h.
    let sparse = inc.join("rocsparse");
    if wanted.split(',').any(|w| w.trim() == "rocsparse") && sparse.join("rocsparse-types.h").exists() {
        scan_types(&sparse.join("rocsparse-types.h"), &mut g.types);
        let mut headers = Vec::new();
        collect_headers(&sparse.join("internal"), &mut headers);
        headers.sort();
        for path in headers {
            println!("cargo:rerun-if-changed={}", path.display());
            g.header(&path, "rocsparse", &["ROCSPARSE_EXPORT"]).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
    if g.functions().next().is_none() {
        panic!("tapir-rocm: no functions found under {} (set ROCM_PATH)", inc.display());
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("wrappers.rs"), g.render()).unwrap();
    std::fs::write(out.join("report.txt"), g.report()).unwrap();
    println!("cargo:rustc-env=TAPIR_ROCM_ROOT={}", root.display());
}

/// Register the opaque handles (`typedef struct _x* name;`, also `const*`) and
/// enums (`typedef enum ... } name;`) declared in a types header.
fn scan_types(path: &Path, types: &mut TypeMap) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut in_enum = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("typedef struct _") && line.ends_with(';') && line.contains('*') {
            if let Some(name) = line.trim_end_matches(';').rsplit(|c: char| c == '*' || c == ' ').next() {
                types.handle(name);
            }
        } else if line.starts_with("typedef enum") {
            in_enum = true;
        } else if in_enum && line.starts_with('}') {
            let name = line.trim_start_matches('}').trim().trim_end_matches(';').trim();
            if !name.is_empty() {
                types.enumeration(name);
            }
            in_enum = false;
        }
    }
}

fn collect_headers(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_headers(&p, out);
        } else if p.extension().is_some_and(|x| x == "h") {
            out.push(p);
        }
    }
}

/// The ROCm math libraries' type vocabulary: integer typedefs, opaque handles
/// and enums (passed as `int`).
fn rocm_types() -> TypeMap {
    let mut m = TypeMap::c_basic();
    m.scalar("rocblas_int", "i32"); // LP64 build of rocBLAS
    m.scalar("rocsparse_int", "i32"); // LP64 build of rocSPARSE
    m.scalar("rocblas_stride", "i64");
    m.scalar("rocblas_size_t", "usize");
    m.scalar("hipblasStride", "i64");
    m.scalar("hipblasLtInt", "i64");
    m.scalar("rocblas_layer_mode_flags", "u32");
    for h in HANDLES {
        m.handle(h);
    }
    for e in ENUMS {
        m.enumeration(e);
    }
    m
}

const HANDLES: &[&str] = &[
    "rocblas_handle", "hipblasHandle_t", "hipsolverHandle_t",
    "hipsolverDnHandle_t", "hipStream_t", "hipsolverDnParams_t",
    "rocsolver_rfinfo", "hipsolverSyevjInfo_t", "hipsolverGesvdjInfo_t",
];

const ENUMS: &[&str] = &[
    "rocblas_operation", "rocblas_fill", "rocblas_diagonal",
    "rocblas_side", "rocblas_datatype", "rocblas_computetype",
    "rocblas_gemm_algo", "rocblas_gemm_flags", "rocblas_layer_mode",
    "rocblas_pointer_mode", "rocblas_atomics_mode", "rocblas_math_mode",
    "rocblas_geam_ex_operation", "rocblas_status", "rocblas_direct",
    "rocblas_storev", "rocblas_svect", "rocblas_evect",
    "rocblas_workmode", "rocblas_eform", "rocblas_erange",
    "rocblas_eorder", "rocblas_esort", "rocblas_srange",
    "rocblas_eval", "rocsolver_rfinfo_mode", "rocblas_pivot",
    "rocsolver_alg_mode", "rocsolver_function", "hipblasAtomicsMode_t",
    "hipblasGemmFlags_t", "hipblasMath_t", "hipblasPointerMode_t",
    "hipsolverDeterministicMode_t", "rocblas_check_numerics_mode", "hipblasOperation_t",
    "hipblasFillMode_t", "hipblasDiagType_t", "hipblasSideMode_t",
    "hipblasDatatype_t", "hipblasComputeType_t", "hipblasGemmAlgo_t",
    "hipblasStatus_t", "hipDataType", "hipsolverStatus_t",
    "hipsolverOperation_t", "hipsolverFillMode_t", "hipsolverSideMode_t",
    "hipsolverEigMode_t", "hipsolverEigType_t", "hipsolverEigRange_t",
    "hipsolverDnFunction_t", "hipsolverAlgMode_t",
];
