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
//! | 4     | [`meta`]         | IVW / MR-Egger with Cochran's Q, I², τ²              |
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
//! All four layers (0–3) are complete. The agent tooling (`stat-tools` wrappers)
//! will be built on top in a separate crate.

// This is a numerical library: many constants (Lanczos/Acklam coefficients,
// statistical reference values) carry full f64 precision deliberately.
#![allow(clippy::excessive_precision)]

pub mod descriptive;
pub mod distribution;
pub mod error;
pub mod meta;
pub mod numeric;
pub mod regression;
pub mod util;

pub use error::{Result, StatError};
pub use meta::{mr_egger, ivw, EggerResult, IvwResult};
