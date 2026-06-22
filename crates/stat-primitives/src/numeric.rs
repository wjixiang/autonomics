//! Layer 0 — numeric primitives: special functions and the delta method.

pub mod delta;
pub mod special;

pub use delta::delta_method;
pub use special::{beta, beta as beta_func, betai, erfc, erf, erfinv, gamma, ln_beta, ln_gamma, std_normal_ppf};
