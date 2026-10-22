// Generate wrappers from the C header, exactly as tapir-rocm does for ROCm:
// the library's own types, plus the shared linalg vocabulary for tags.
fn main() {
    let mut types = tapir_gen::TypeMap::c_basic();
    types.handle("fake_handle").enumeration("fake_operation").scalar("fake_int", "i32");
    let mut g = tapir_gen::Generator::new(types);
    g.rules.parse_text(tapir_vocab::LINALG.args).unwrap();
    g.header("c/fakeblas.h", "fakeblas", &["FAKE_EXPORT"]).unwrap();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("wrappers.rs"), g.render()).unwrap();
    println!("cargo:rerun-if-changed=c/fakeblas.h");
}
