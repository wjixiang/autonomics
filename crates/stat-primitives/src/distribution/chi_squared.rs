//! The chi-squared distribution `χ²(k)` (degrees of freedom `k > 0`).
//!
//! A special case of the gamma distribution; the CDF is the regularized lower
//! incomplete gamma `P(k/2, x/2)`.

use super::traits::Distribution;
use crate::distribution::inverse_cdf;
use crate::numeric::special::{gammp, gammq, ln_gamma};
use std::f64::consts;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquared {
    pub k: f64, // degrees of freedom
}

impl ChiSquared {
    /// `k` must be positive.
    pub const fn new(k: f64) -> Self {
        Self { k }
    }
}

impl Distribution for ChiSquared {
    fn pdf(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 0.0;
        }
        let half_k = self.k / 2.0;
        // f(x) = x^{k/2−1} e^{−x/2} / (2^{k/2} Γ(k/2))
        //      = exp((k/2−1)·ln x − x/2 − (k/2)·ln2 − lnΓ(k/2)).
        ((half_k - 1.0) * x.ln() - x / 2.0 - half_k * consts::LN_2 - ln_gamma(half_k)).exp()
    }

    fn cdf(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        gammp(self.k / 2.0, x / 2.0)
    }

    fn sf(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 1.0;
        }
        gammq(self.k / 2.0, x / 2.0)
    }

    fn ppf(&self, p: f64) -> f64 {
        // χ² support is [0, ∞); seed the bracket near the mean (k) and bisect.
        let seed = self.k.max(1.0);
        inverse_cdf(|x| self.cdf(x), p, seed * 1e-8, seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn cdf_at_zero_and_known() {
        let d = ChiSquared::new(5.0);
        assert_eq!(d.cdf(0.0), 0.0);
        // chi2.cdf(5, df=5) = P(2.5, 2.5) ≈ 0.584120 (mean is slightly above the
        // 0.5-median 4.351, so the CDF at 5 must exceed 0.5).
        assert!(approx_eq(d.cdf(5.0), 0.5841198130043434, 1e-9));
        // Textbook critical values: median ≈ 4.35146, 95th percentile ≈ 11.0705.
        assert!(approx_eq(d.ppf(0.5), 4.351460191096394, 1e-4));
        assert!(approx_eq(d.ppf(0.95), 11.070497693516449, 1e-4));
        assert!(approx_eq(d.cdf(11.070497693516449), 0.95, 1e-4));
        assert!(approx_eq(d.sf(5.0), 1.0 - 0.5841198130043434, 1e-9));
    }

    #[test]
    fn pdf_positive_and_zero_below_zero() {
        let d = ChiSquared::new(3.0);
        assert_eq!(d.pdf(-1.0), 0.0);
        // pdf integrates; just check it is positive on the support.
        assert!(d.pdf(1.0) > 0.0);
    }

    #[test]
    fn ppf_roundtrip() {
        let d = ChiSquared::new(7.0);
        for &p in &[0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
            let x = d.ppf(p);
            assert!(approx_eq(d.cdf(x), p, 1e-6), "p={p} x={x} cdf={}", d.cdf(x));
        }
    }
}
