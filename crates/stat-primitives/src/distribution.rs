//! Layer 2 — probability distributions.
//!
//! All continuous distributions implement the [`Distribution`] trait with
//! `pdf`, `cdf`, `sf` (survival) and `ppf` (quantile / inverse CDF). The
//! engines live in [`crate::numeric::special`]:
//!
//! - Normal CDF uses [`erfc`](crate::numeric::special::erfc); its PPF uses the
//!   closed-form Acklam + Halley [`std_normal_ppf`].
//! - Student-t / F / Beta CDFs use the regularized incomplete beta
//!   [`betai`](crate::numeric::special::betai).
//! - χ² CDF uses the lower incomplete gamma
//!   [`gammp`](crate::numeric::special::gammp).
//!
//! The non-normal PPFs fall back to bracketed bisection via [`inverse_cdf`].

pub mod beta;
pub mod chi_squared;
pub mod f;
pub mod normal;
pub mod t;
pub mod traits;

pub use beta::Beta;
pub use chi_squared::ChiSquared;
pub use f::FDistribution;
pub use normal::Normal;
pub use t::StudentT;
pub use traits::Distribution;

/// Bracketed bisection for the inverse CDF of a continuous, strictly
/// increasing CDF.
///
/// `cdf` must be monotonic non-decreasing; `p ∈ (0, 1)`. The bracket `[lo, hi]`
/// is expanded outward (geometrically) until `cdf(lo) ≤ p ≤ cdf(hi)`, then
/// bisected to ~`1e-12` absolute precision on the quantile.
pub(crate) fn inverse_cdf(cdf: impl Fn(f64) -> f64, p: f64, mut lo: f64, mut hi: f64) -> f64 {
    // Expand the bracket until it actually straddles p.
    let mut guard = 0;
    while cdf(hi) < p && hi.is_finite() {
        hi *= 2.0;
        guard += 1;
        if guard > 200 {
            break;
        }
    }
    guard = 0;
    while cdf(lo) > p && lo.is_finite() {
        lo *= 0.5;
        guard += 1;
        if guard > 200 {
            break;
        }
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if mid == lo || mid == hi {
            break;
        }
        if cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}
