//! Distribution functions reproducing R's `stats::pnorm` / `pt` / `pchisq` /
//! `pf` and their inverses, plus normal sampling, on top of `statrs`.
//!
//! R uses `lower.tail = TRUE` (default) for the CDF and `lower.tail = FALSE`
//! for the survival function. This module exposes both as `_lower` / `_sf`.
//!
//! All functions return `f64::NAN` for invalid parameters (df ≤ 0, etc.),
//! matching R's tendency to return `NaN`.

use statrs::distribution::{ChiSquared, ContinuousCDF, FisherSnedecor, Normal, StudentsT};

// ---------------------------------------------------------------------------
// Normal  (R: pnorm / qnorm)
// ---------------------------------------------------------------------------

/// `pnorm(q)` — standard-normal CDF Φ(q).
pub fn pnorm_lower(q: f64) -> f64 {
    match Normal::new(0.0, 1.0) {
        Ok(d) => d.cdf(q),
        Err(_) => f64::NAN,
    }
}

/// `pnorm(q, lower.tail = FALSE)` — standard-normal survival 1 − Φ(q).
pub fn pnorm_sf(q: f64) -> f64 {
    match Normal::new(0.0, 1.0) {
        Ok(d) => d.sf(q),
        Err(_) => f64::NAN,
    }
}

/// Two-sided normal p-value: `2 * pnorm(-|z|)` ≡ `2 * pnorm(|z|, lower.tail=FALSE)`.
pub fn pnorm_two_sided(z: f64) -> f64 {
    2.0 * pnorm_sf(z.abs())
}

/// `qnorm(p)` — standard-normal quantile (inverse CDF).
pub fn qnorm_lower(p: f64) -> f64 {
    match Normal::new(0.0, 1.0) {
        Ok(d) => d.inverse_cdf(p),
        Err(_) => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// Student's t  (R: pt / qt)
// ---------------------------------------------------------------------------

/// `pt(q, df)`.
pub fn pt_lower(q: f64, df: f64) -> f64 {
    match StudentsT::new(0.0, 1.0, df) {
        Ok(d) => d.cdf(q),
        Err(_) => f64::NAN,
    }
}

/// `pt(q, df, lower.tail = FALSE)`.
pub fn pt_sf(q: f64, df: f64) -> f64 {
    match StudentsT::new(0.0, 1.0, df) {
        Ok(d) => d.sf(q),
        Err(_) => f64::NAN,
    }
}

/// Two-sided t p-value: `2 * pt(|t|, df, lower.tail = FALSE)`.
pub fn pt_two_sided(t: f64, df: f64) -> f64 {
    2.0 * pt_sf(t.abs(), df)
}

/// `qt(p, df)`.
pub fn qt_lower(p: f64, df: f64) -> f64 {
    match StudentsT::new(0.0, 1.0, df) {
        Ok(d) => d.inverse_cdf(p),
        Err(_) => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// Chi-squared  (R: pchisq / qchisq)
// ---------------------------------------------------------------------------

/// `pchisq(q, df)`.
pub fn pchisq_lower(q: f64, df: f64) -> f64 {
    match ChiSquared::new(df) {
        Ok(d) => d.cdf(q),
        Err(_) => f64::NAN,
    }
}

/// `pchisq(q, df, lower.tail = FALSE)`.
pub fn pchisq_sf(q: f64, df: f64) -> f64 {
    match ChiSquared::new(df) {
        Ok(d) => d.sf(q),
        Err(_) => f64::NAN,
    }
}

/// `qchisq(p, df)`.
pub fn qchisq_lower(p: f64, df: f64) -> f64 {
    match ChiSquared::new(df) {
        Ok(d) => d.inverse_cdf(p),
        Err(_) => f64::NAN,
    }
}

/// `qchisq(p, df, lower.tail = FALSE)`.
pub fn qchisq_sf(p: f64, df: f64) -> f64 {
    match ChiSquared::new(df) {
        Ok(d) => d.inverse_cdf(1.0 - p),
        Err(_) => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// F  (R: pf / qf)
// ---------------------------------------------------------------------------

/// `pf(q, df1, df2)`.
pub fn pf_lower(q: f64, df1: f64, df2: f64) -> f64 {
    match FisherSnedecor::new(df1, df2) {
        Ok(d) => d.cdf(q),
        Err(_) => f64::NAN,
    }
}

/// `pf(q, df1, df2, lower.tail = FALSE)`.
pub fn pf_sf(q: f64, df1: f64, df2: f64) -> f64 {
    match FisherSnedecor::new(df1, df2) {
        Ok(d) => d.sf(q),
        Err(_) => f64::NAN,
    }
}

/// `qf(p, df1, df2)`.
pub fn qf_lower(p: f64, df1: f64, df2: f64) -> f64 {
    match FisherSnedecor::new(df1, df2) {
        Ok(d) => d.inverse_cdf(p),
        Err(_) => f64::NAN,
    }
}

/// `qf(p, df1, df2, lower.tail = FALSE)`.
pub fn qf_sf(p: f64, df1: f64, df2: f64) -> f64 {
    match FisherSnedecor::new(df1, df2) {
        Ok(d) => d.inverse_cdf(1.0 - p),
        Err(_) => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-300)
    }

    #[test]
    fn normal_basics() {
        // Φ(0)=0.5, Φ(1.96)≈0.975, 2*Φ(-1.96)≈0.05
        assert!(approx(pnorm_lower(0.0), 0.5, 1e-12));
        assert!(approx(pnorm_lower(1.95996398454005), 0.975, 1e-9));
        assert!(approx(pnorm_two_sided(1.95996398454005), 0.05, 1e-9));
        // qnorm round-trip
        assert!(approx(qnorm_lower(0.975), 1.95996398454005, 1e-9));
    }

    #[test]
    fn t_basics() {
        // pt(2, 10) = 0.963306 (R); 2*pt(-2,10) = 0.07338803
        assert!(approx(pt_lower(2.0, 10.0), 0.963306, 1e-6));
        assert!(approx(pt_two_sided(2.0, 10.0), 0.07338803, 1e-6));
    }

    #[test]
    fn chisq_basics() {
        // pchisq(3.84, 1) ≈ 0.95; sf ≈ 0.05
        assert!(approx(pchisq_lower(3.84145882069412, 1.0), 0.95, 1e-8));
        assert!(approx(pchisq_sf(3.84145882069412, 1.0), 0.05, 1e-8));
        assert!(approx(qchisq_lower(0.95, 1.0), 3.84145882069412, 1e-7));
    }

    #[test]
    fn f_basics() {
        // pf(4.96, 1, 10) ≈ 0.95; qf(0.95,1,10) ≈ 4.9646
        assert!(approx(pf_lower(4.964603, 1.0, 10.0), 0.95, 1e-5));
        assert!(approx(qf_lower(0.95, 1.0, 10.0), 4.964603, 1e-5));
        // qf(p, lower.tail=FALSE) sanity: qf_sf(0.05,1,10) ≈ 4.9646
        assert!(approx(qf_sf(0.05, 1.0, 10.0), 4.964603, 1e-4));
    }

    #[test]
    fn f_extreme_tail() {
        // R: qf(1.81201e-08, 1, 338902, lower.tail=FALSE) = 31.68777
        // (genome-wide-significant SNP → R² ≈ 9.4e-5). statrs must match.
        let q = qf_sf(1.81201e-08, 1.0, 338902.0);
        assert!((q - 31.68777).abs() < 1e-2, "qf_sf extreme tail = {q}");
    }

    #[test]
    fn pf_sf_extreme_matches_r() {
        // R: pf(85, 1, 338902, lower.tail=FALSE) = 2.999967e-20.
        let s = pf_sf(85.0, 1.0, 338902.0);
        assert!((s - 3e-20).abs() < 1e-21, "pf_sf(85,1,338902) = {s}");
    }

    #[test]
    fn pf_sf_huge_f() {
        // Large F relative to df2 — statrs must still return a finite ~0.
        for &f in &[1e3, 1e5, 338827.0, 1e10] {
            let s = pf_sf(f, 1.0, 338902.0);
            assert!(s.is_finite(), "pf_sf({f},1,338902) not finite: {s}");
        }
    }
}
