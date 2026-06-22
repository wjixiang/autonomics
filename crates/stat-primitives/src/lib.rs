//! `stat-primitives` — a dependency-free statistics primitives library.
//!
//! The crate is organised in layers, each building on the one below:
//!
//! | Layer | Module           | Contents                                              |
//! |-------|------------------|-------------------------------------------------------|
//! | 0     | [`numeric`]      | Special functions (gamma, beta, erf), delta method    |
//! | 1     | [`descriptive`]  | Mean/variance/correlation, weighted stats, ranks      |
//! | 2     | [`distribution`] | Normal / Student-t / χ² / F / Beta (pdf, cdf, sf, ppf)|
//! | 3     | [`regression`]   | OLS / WLS with SE, t, p, R² (the MR primitive)        |
//! | 4+    | _(future)_       | Meta-analysis (IVW / Egger)                           |
//!
//! Design principles:
//! - **Zero external dependencies.** Every special function and the error type
//!   are hand-written, so the crate is fully self-contained and easy to audit.
//! - **Slice interfaces.** Primitives take `&[f64]` rather than owning
//!   containers, leaving memory management to the caller.
//! - **Compensated summation** throughout ([`util`]) to limit accumulation
//!   error in variance/covariance computations.
//! - **Explicit errors.** Empty input, length mismatches, and bad weights are
//!   reported via [`StatError`], not silent `NaN`s.
//!
//! Higher layers (meta-analysis: IVW / Egger) and the agent tooling built on
//! top are added in later increments; Layers 0–3 are complete.

// This is a numerical library: many constants (Lanczos/Acklam coefficients,
// statistical reference values) carry full f64 precision deliberately.
#![allow(clippy::excessive_precision)]

pub mod descriptive;
pub mod distribution;
pub mod error;
pub mod numeric;
pub mod regression;
pub mod util;

pub use error::{Result, StatError};
