//! The Beta distribution on `[0, 1]` with shape parameters `a, b > 0`.
//!
//! CDF is the regularized incomplete beta `I_x(a, b)`.

use super::traits::Distribution;
use crate::distribution::inverse_cdf;
use crate::numeric::special::{betai, ln_beta};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beta {
    pub a: f64,
    pub b: f64,
}

impl Beta {
    /// Both shape parameters must be positive.
    pub const fn new(a: f64, b: f64) -> Self {
        Self { a, b }
    }
}

impl Distribution for Beta {
    fn pdf(&self, x: f64) -> f64 {
        if !(0.0..=1.0).contains(&x) {
            return 0.0;
        }
        // f(x) = x^{a−1} (1−x)^{b−1} / B(a, b).
        ((self.a - 1.0) * x.ln() + (self.b - 1.0) * (1.0 - x).ln() - ln_beta(self.a, self.b)).exp()
    }

    fn cdf(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        betai(self.a, self.b, x)
    }

    fn ppf(&self, p: f64) -> f64 {
        // Support [0, 1]; mode (a−1)/(a+b−2) is a decent seed.
        let seed = if self.a > 1.0 && self.b > 1.0 {
            (self.a - 1.0) / (self.a + self.b - 2.0)
        } else {
            0.5
        };
        let lo = seed.min(0.5) * 1e-6;
        let hi = (seed.max(0.5)) + (1.0 - seed.max(0.5)) * (1.0 - 1e-6);
        inverse_cdf(|x| self.cdf(x), p, lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn cdf_known_values() {
        let d = Beta::new(2.0, 3.0);
        assert_eq!(d.cdf(0.0), 0.0);
        assert_eq!(d.cdf(1.0), 1.0);
        // betai(2,3,0.5) = 0.6875.
        assert!(approx_eq(d.cdf(0.5), 0.6875, 1e-9));
        // Symmetric Beta(3,3): cdf(0.5) = 0.5.
        let s = Beta::new(3.0, 3.0);
        assert!(approx_eq(s.cdf(0.5), 0.5, 1e-12));
    }

    #[test]
    fn pdf_support() {
        let d = Beta::new(2.0, 5.0);
        assert_eq!(d.pdf(-0.1), 0.0);
        assert_eq!(d.pdf(1.1), 0.0);
        // pdf integrates to 1 over [0,1] (left Riemann sanity check).
        let n = 100_000;
        let h = 1.0 / n as f64;
        let integral: f64 = (0..n).map(|i| d.pdf((i as f64 + 0.5) * h) * h).sum();
        assert!(approx_eq(integral, 1.0, 1e-3));
    }

    #[test]
    fn ppf_roundtrip() {
        let d = Beta::new(2.0, 5.0);
        for &p in &[0.1, 0.25, 0.5, 0.75, 0.9] {
            let x = d.ppf(p);
            assert!(approx_eq(d.cdf(x), p, 1e-6), "p={p} x={x}");
        }
    }
}
