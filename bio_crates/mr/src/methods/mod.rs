//! MR estimators ported from `R/mr.R`, `R/mr_mode.R`. Each estimator mirrors
//! its R counterpart's signature `(b_exp, b_out, se_exp, se_out, parameters)`
//! and returns an [`crate::MrEstimate`].
//!
//! Deterministic estimators (Wald, IVW family, Egger point estimate) are
//! bit-aligned to R's golden outputs; median / mode / Egger-bootstrap standard
//! errors depend on an RNG stream and are validated for convergence only.

pub mod egger;
pub mod ivw;
pub mod median;
pub mod mode;
pub mod wald;

pub use egger::mr_egger_regression;
pub use ivw::{mr_ivw, mr_ivw_fe, mr_ivw_mre, mr_uwr};
pub use median::{mr_penalised_weighted_median, mr_simple_median, mr_weighted_median};
pub use mode::{mr_simple_mode, mr_simple_mode_nome, mr_weighted_mode, mr_weighted_mode_nome};
pub use wald::mr_wald_ratio;
