//! Argument vocabularies for tapir wrappers.
//!
//! tapir's core gives arguments no meaning; a vocabulary does, for one family
//! of APIs. It is plain data in two parts:
//!
//! - `args`: parameter rules for `tapir-gen` (`selector action` lines, see
//!   `tapir_gen::Rules`), which choose capture modes and tags from parameter
//!   names and C kinds. Use it in a `build.rs`:
//!   `g.rules.parse_text(tapir_vocab::LINALG.args)?`, or from the command line
//!   with `tapir-gen --rules crates/tapir-vocab/src/linalg.args`.
//! - `key`: default key rules for the runtime (see `tapir::KeyPolicy`), for
//!   the preload library's setup: `b.default_key(tapir_vocab::LINALG.key)`.
//!
//! Add a vocabulary by adding a `<name>.args` file and a constant here.

/// One vocabulary.
#[derive(Debug)]
pub struct Vocab {
    pub name: &'static str,
    /// `tapir-gen` parameter rules.
    pub args: &'static str,
    /// Default `tapir::KeyPolicy` rules.
    pub key: &'static str,
}

/// BLAS, LAPACK and similar dense/sparse linear-algebra APIs: sizes bucketed
/// by bit length, modes and strides exact, scalars ignored.
pub const LINALG: Vocab = Vocab {
    name: "linalg",
    args: include_str!("linalg.args"),
    key: "@dim:log2; @count:log2; @scalar:drop",
};

/// Every vocabulary in this crate.
pub const ALL: &[&Vocab] = &[&LINALG];

/// Look a vocabulary up by name.
pub fn get(name: &str) -> Option<&'static Vocab> {
    ALL.iter().copied().find(|v| v.name == name)
}
