//! Student's t-distribution with `df > 0` degrees of freedom.
//!
//! CDF expressed via the regularized incomplete beta: for `t ≥ 0`,
//! `F(t) = 1 − ½·I_x(ν/2, ½)` with `x = ν/(ν+t²)`; for `t < 0` use symmetry.

use super::traits::Distribution;
use crate::distribution::inverse_cdf;
use crate::numeric::special::{betai, ln_beta};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StudentT {
    pub df: f64, // degrees of freedom
}

impl StudentT {
    /// `df` must be positive.
    pub const fn new(df: f64) -> Self {
        Self { df }
    }
}

impl Distribution for StudentT {
    fn pdf(&self, t: f64) -> f64 {
        // f(t) = Γ((ν+1)/2) / (√(νπ) Γ(ν/2)) · (1 + t²/ν)^{−(ν+1)/2}.
        // Using B(ν/2, ½) = Γ(ν/2)Γ(½)/Γ((ν+1)/2) ⇒ normalizer = exp(−ln_beta(ν/2,½)) / √ν.
        let nu = self.df;
        let norm = (-ln_beta(nu / 2.0, 0.5)).exp() / nu.sqrt();
        norm * (1.0 + t * t / nu).powf(-0.5 * (nu + 1.0))
    }

    fn cdf(&self, t: f64) -> f64 {
        let nu = self.df;
        let x = nu / (nu + t * t);
        let half_ibeta = 0.5 * betai(nu / 2.0, 0.5, x);
        if t >= 0.0 {
            1.0 - half_ibeta
        } else {
            half_ibeta
        }
    }

    fn ppf(&self, p: f64) -> f64 {
        // t is symmetric about 0; solve the positive half and reflect.
        if p == 0.5 {
            return 0.0;
        }
        if p > 0.5 {
            inverse_cdf(|t| self.cdf(t), p, 0.0, 1.0)
        } else {
            -inverse_cdf(|t| self.cdf(t), 1.0 - p, 0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn cdf_median_and_symmetry() {
        let d = StudentT::new(10.0);
        assert_eq!(d.cdf(0.0), 0.5);
        // Symmetry: F(−t) = 1 − F(t).
        let t = 1.5;
        assert!(approx_eq(d.cdf(-t), 1.0 - d.cdf(t), 1e-12));
    }

    #[test]
    fn cdf_known_values() {
        let d = StudentT::new(10.0);
        // t.cdf(0, 10) = 0.5; t.cdf(1.812461, 10) = 0.95 (the one-sided 0.05 critical value).
        assert!(approx_eq(d.cdf(1.8124611228147604), 0.95, 1e-5));
        // pdf at 0 = Γ(5.5)/(√(10π)·Γ(5)) ≈ 0.3891084 (verified against the exact
        // Cauchy pdf(0)=1/π at df=1 and pdf integrating to 1).
        assert!(approx_eq(d.pdf(0.0), 0.3891083840227016, 1e-7));
    }

    #[test]
    fn pdf_matches_known_closed_forms() {
        // Cauchy (df=1): pdf(0) = 1/π.
        assert!(approx_eq(StudentT::new(1.0).pdf(0.0), 1.0 / std::f64::consts::PI, 1e-9));
        // df=2: pdf(0) = 1/(2√2).
        assert!(approx_eq(
            StudentT::new(2.0).pdf(0.0),
            1.0 / (2.0 * std::f64::consts::SQRT_2),
            1e-9
        ));
    }

    #[test]
    fn ppf_roundtrip_and_symmetric() {
        let d = StudentT::new(8.0);
        for &p in &[0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
            let x = d.ppf(p);
            assert!(approx_eq(d.cdf(x), p, 1e-6), "p={p} x={x}");
        }
        // ppf is odd-symmetric.
        assert!(approx_eq(d.ppf(0.95), -d.ppf(0.05), 1e-6));
    }

    #[test]
    fn converges_to_normal_for_large_df() {
        // For large df the t-dist approaches N(0,1): 0.975 quantile → 1.96.
        let d = StudentT::new(1.0e6);
        assert!(approx_eq(d.ppf(0.975), 1.959963984540054, 1e-3));
    }
}
