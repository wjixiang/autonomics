//! The normal distribution `N(μ, σ²)`.

use super::traits::Distribution;
use crate::numeric::special::{erfc, std_normal_ppf};
use std::f64::consts::{PI, SQRT_2};

/// Normal distribution with mean `mu` and standard deviation `sigma` (`sigma > 0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normal {
    pub mu: f64,
    pub sigma: f64,
}

impl Normal {
    /// Create with a mean and standard deviation. `sigma` must be positive.
    pub const fn new(mu: f64, sigma: f64) -> Self {
        Self { mu, sigma }
    }

    /// Standard normal `N(0, 1)`.
    pub const fn standard() -> Self {
        Self::new(0.0, 1.0)
    }

    /// Standardized z-score `(x − μ) / σ`.
    pub fn z(&self, x: f64) -> f64 {
        (x - self.mu) / self.sigma
    }
}

impl Distribution for Normal {
    fn pdf(&self, x: f64) -> f64 {
        let z = self.z(x);
        (-0.5 * z * z).exp() / (self.sigma * (2.0 * PI).sqrt())
    }

    fn cdf(&self, x: f64) -> f64 {
        // Φ(z) = ½·erfc(−z/√2), accurate in both tails.
        0.5 * erfc(-self.z(x) / SQRT_2)
    }

    fn sf(&self, x: f64) -> f64 {
        0.5 * erfc(self.z(x) / SQRT_2)
    }

    fn ppf(&self, p: f64) -> f64 {
        self.mu + self.sigma * std_normal_ppf(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn standard_normal_known_values() {
        let n = Normal::standard();
        assert_eq!(n.cdf(0.0), 0.5);
        assert!(approx_eq(n.pdf(0.0), 0.3989422804014327, 1e-12));
        assert!(approx_eq(n.cdf(1.96), 0.9750021048517795, 1e-9));
        assert!(approx_eq(n.cdf(-1.96), 0.02499789514822044, 1e-9));
        assert!(approx_eq(n.sf(1.96), 1.0 - 0.9750021048517795, 1e-9));
    }

    #[test]
    fn standard_normal_ppf_roundtrip() {
        let n = Normal::standard();
        for &p in &[0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            assert!(approx_eq(n.cdf(n.ppf(p)), p, 1e-9));
        }
        assert!(approx_eq(n.ppf(0.975), 1.959963984540054, 1e-8));
    }

    #[test]
    fn nonstandard_shifts_and_scales() {
        let n = Normal::new(5.0, 2.0);
        assert!(approx_eq(n.cdf(5.0), 0.5, 1e-12)); // mean → 0.5
        // 5 + 2·1.96 ≈ 8.92 is the 97.5th percentile.
        assert!(approx_eq(n.ppf(0.975), 5.0 + 2.0 * 1.959963984540054, 1e-8));
        assert!(approx_eq(n.cdf(5.0 + 2.0 * 1.96), 0.9750021048517795, 1e-9));
    }
}
