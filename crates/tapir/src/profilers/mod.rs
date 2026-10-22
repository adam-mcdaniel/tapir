//! Built-in profilers. Each is selectable by name in `TAPIR_PROFILERS`.

pub mod bbv;
pub mod summary;
pub mod trace;

pub use bbv::Bbv;
pub use summary::Summary;
pub use trace::Trace;

pub(crate) fn register_builtins(b: &mut crate::Builder) {
    b.factory("bbv", Bbv::from_opts);
    b.factory("summary", Summary::from_opts);
    b.factory("trace", Trace::from_opts);
}
