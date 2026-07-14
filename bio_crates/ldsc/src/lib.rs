//! `ldsc` — LD Score Regression for SNP-heritability (h²) and (later) genetic
//! correlation, in pure Rust.
//!
//! Linear algebra runs on [`faer`] (no LAPACK/MKL backend). Data enters as a
//! DataFusion [`DataFrame`](datafusion::DataFrame). The headline entry point is
//! [`hsq::estimate_h2`].
//!
//! | Module      | Contents                                                     |
//! |-------------|--------------------------------------------------------------|
//! | [`linalg`]  | faer weighted-least-squares / SPD-solve helpers              |
//! | [`jackknife`]| Block jackknife (`LstsqJackknifeFast`)                      |
//! | [`irwls`]   | Two-pass IRWLS + the `Hsq` reweighting function              |
//! | [`ingest`]  | DataFrame → per-SNP faer vectors                             |
//! | [`hsq`]     | The h² driver + result type                                  |

pub mod hsq;
pub mod ingest;
pub mod irwls;
pub mod jackknife;
pub mod linalg;

use datafusion::error::DataFusionError;

/// Error type for the `ldsc` crate.
#[derive(Debug, thiserror::Error)]
pub enum LdscError {
    /// Shape / size mismatch between vectors or matrices.
    #[error("dimension mismatch: {0}")]
    DimensionMismatch(String),
    /// An input value is invalid (empty data, non-numeric column, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// A numerical linear-algebra operation failed (e.g. LLᵀ factorisation).
    #[error("linear algebra error: {0}")]
    Linalg(String),
    /// Transparent passthrough for DataFusion errors.
    #[error(transparent)]
    DataFusion(#[from] DataFusionError),
}

/// Crate-local `Result` alias.
pub type Result<T> = std::result::Result<T, LdscError>;
