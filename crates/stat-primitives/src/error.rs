//! Error type for the statistics primitives crate.
//!
//! Hand-rolled (no `thiserror`) to keep the crate dependency-free. Mirrors the
//! shape used elsewhere in the workspace (`agentik-types::errors`): an enum with
//! a [`Result`](type.Result.html) alias at the crate root.

use std::fmt;

/// Errors returned by statistics primitives.
///
/// Variants distinguish *statistical* invalidity (empty input, mismatched
/// lengths, negative weights) from generic bad arguments. Numerical failures
/// such as non-convergence are not represented here yet — they appear once the
/// regression/meta layers need them.
#[derive(Debug, Clone, PartialEq)]
pub enum StatError {
    /// The input slice contained no values.
    EmptyInput,
    /// Fewer values than the operation requires.
    InsufficientData {
        /// Minimum number of values the operation needs.
        min: usize,
        /// Number of values actually supplied.
        actual: usize,
    },
    /// Two parallel slices had different lengths.
    LengthMismatch {
        /// Length of the first slice.
        a: usize,
        /// Length of the second slice.
        b: usize,
    },
    /// One or more weights were negative or all weights summed to zero.
    InvalidWeights,
    /// A quantile probability was outside `[0, 1]`.
    InvalidQuantile(f64),
    /// The design matrix was singular / not invertible (perfect collinearity).
    SingularMatrix,
    /// Catch-all for malformed arguments not covered above.
    InvalidInput(String),
}

impl fmt::Display for StatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatError::EmptyInput => write!(f, "input is empty: at least one value required"),
            StatError::InsufficientData { min, actual } => {
                write!(
                    f,
                    "insufficient data: need at least {min} value(s), got {actual}"
                )
            }
            StatError::LengthMismatch { a, b } => {
                write!(f, "length mismatch: {a} vs {b}")
            }
            StatError::InvalidWeights => {
                write!(f, "invalid weights: must be non-negative and not all zero")
            }
            StatError::InvalidQuantile(q) => write!(f, "invalid quantile: {q} is outside [0, 1]"),
            StatError::SingularMatrix => {
                write!(f, "singular matrix: design matrix is not invertible")
            }
            StatError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for StatError {}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, StatError>;
