//! The F-distribution `F(d1, d2)` with numerator/denominator degrees of freedom.
//!
//! CDF: `F(x) = I_{d1·x/(d1·x + d2)}(d1/2, d2/2)` for `x ≥ 0`.

use super::traits::Distribution;
use crate::distribution::inverse_cdf;
use crate::numeric::special::{betai, ln_beta};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FDistribution {
    pub d1: f64,
    pub d2: f64,
}

impl FDistribution {
    /// Both degrees of freedom must be positive.
    pub const fn new(d1: f64, d2: f64) -> Self {
        Self { d1, d2 }
    }
}

impl Distribution for FDistribution {
    fn pdf(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        // f(x) = (d1/d2)^{d1/2} x^{d1/2−1} (1 + d1 x/d2)^{−(d1+d2)/2} / B(d1/2, d2/2)
        let a = self.d1 / 2.0;
        let b = self.d2 / 2.0;
        let log_pdf = a * (self.d1 / self.d2).ln() + (a - 1.0) * x.ln()
            - (a + b) * (1.0 + self.d1 * x / self.d2).ln()
            - ln_beta(a, b);
        log_pdf.exp()
    }

    fn cdf(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        let arg = self.d1 * x / (self.d1 * x + self.d2);
        betai(self.d1 / 2.0, self.d2 / 2.0, arg)
    }

    fn ppf(&self, p: f64) -> f64 {
        // F support is (0, ∞); the mean ≈ d2/(d2−2) for d2 > 2, seed near 1.
        let seed = (self.d2 / (self.d2 - 2.0)).max(1.0);
        inverse_cdf(|x| self.cdf(x), p, 1e-12, seed)
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
        let d = FDistribution::new(5.0, 10.0);
        assert_eq!(d.cdf(0.0), 0.0);
        // f.cdf(1, 5, 10) = betai(2.5, 5, 1/3) ≈ 0.534881 (verified by
        // independent numerical integration in special::tests).
        assert!(approx_eq(d.cdf(1.0), 0.5348805734624049, 1e-8));
        // Textbook 95th percentile F(0.95; 5, 10) ≈ 3.3258.
        assert!(approx_eq(d.ppf(0.95), 3.3258249871670138, 1e-3));
        assert!(approx_eq(d.cdf(3.3258249871670138), 0.95, 1e-4));
        assert!(approx_eq(d.sf(1.0), 1.0 - 0.5348805734624049, 1e-9));
    }

    #[test]
    fn pdf_zero_or_negative() {
        let d = FDistribution::new(4.0, 6.0);
        assert_eq!(d.pdf(0.0), 0.0);
        assert_eq!(d.pdf(-1.0), 0.0);
        assert!(d.pdf(1.0) > 0.0);
    }

    #[test]
    fn ppf_roundtrip() {
        let d = FDistribution::new(6.0, 8.0);
        for &p in &[0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
            let x = d.ppf(p);
            assert!(approx_eq(d.cdf(x), p, 1e-6), "p={p} x={x} cdf={}", d.cdf(x));
        }
    }
}
