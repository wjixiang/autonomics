//! Layer 4 — meta-analysis: IVW and MR-Egger for Mendelian Randomization.
//!
//! This layer composes the Layer 3 [`wls`](crate::regression::wls) primitive into
//! the two most common two-sample MR estimators:
//!
//! * **IVW** (inverse-variance weighted): WLS through the origin. Each SNP's
//!   weight is `1/SE²`. Both fixed-effect and random-effects
//!   (DerSimonian–Laird τ²) variants are provided.
//!
//! * **MR-Egger**: WLS with an intercept. The slope is the causal estimate
//!   (adjusted for directional pleiotropy); the intercept tests the STE
//!   (strength and independence of instruments) assumption.
//!
//! Heterogeneity is assessed with Cochran's Q, I², and DerSimonian–Laird τ².

use crate::distribution::{ChiSquared, Distribution};
use crate::error::{Result, StatError};
use crate::regression::wls;
use crate::util::compensated_sum;

/// Result of an IVW Mendelian randomization analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct IvwResult {
    /// Causal effect estimate (IVW slope).
    pub estimate: f64,
    /// Standard error of the estimate.
    pub se: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Cochran's Q statistic for heterogeneity.
    pub q_statistic: f64,
    /// Q p-value (χ² with k − 1 df).
    pub q_p_value: f64,
    /// I² heterogeneity statistic (percentage, 0–100).
    pub i_squared: f64,
    /// DerSimonian–Laird τ² between-study variance.
    pub tau_squared: f64,
    /// Number of SNPs.
    pub n_snps: usize,
}

/// Result of an MR-Egger Mendelian randomization analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct EggerResult {
    /// Egger intercept (average pleiotropy estimate).
    pub intercept: f64,
    /// Standard error of the intercept.
    pub intercept_se: f64,
    /// Two-sided p-value for the intercept (H₀: no pleiotropy).
    pub intercept_p_value: f64,
    /// Causal effect estimate (Egger slope).
    pub slope: f64,
    /// Standard error of the slope.
    pub slope_se: f64,
    /// Two-sided p-value for the slope.
    pub slope_p_value: f64,
    /// Cochran's Q statistic.
    pub q_statistic: f64,
    /// Q p-value (χ² with k − 2 df).
    pub q_p_value: f64,
    /// I² heterogeneity statistic (percentage, 0–100).
    pub i_squared: f64,
    /// τ² between-study variance.
    pub tau_squared: f64,
    /// Number of SNPs.
    pub n_snps: usize,
}

/// Inverse-variance weighted (IVW) Mendelian randomization.
///
/// Performs WLS regression of `beta_outcome` on `beta_exposure` through the
/// origin (no intercept) with weights `1/SE_outcome²`.
///
/// If `random_effects` is `true`, first fits the fixed-effect model to obtain
/// Cochran's Q and DerSimonian–Laird τ², then refits with adjusted weights
/// `1/(SE² + τ²)`.
pub fn ivw(
    beta_exposure: &[f64],
    beta_outcome: &[f64],
    se_outcome: &[f64],
    random_effects: bool,
) -> Result<IvwResult> {
    validate_mr_input(beta_exposure, beta_outcome, se_outcome)?;
    let k = beta_exposure.len();
    let weights: Vec<f64> = se_outcome.iter().map(|s| 1.0 / (s * s)).collect();

    // Fixed-effect fit through origin.
    // For a through-origin WLS, RSS = Σ wᵢ (Yᵢ − β̂·Xᵢ)² = Cochran's Q.
    let fit = wls(&[beta_exposure], beta_outcome, &weights, false)?;

    let q = fit.rss;
    let df = (k - 1) as f64;
    let q_p_value = chi2_sf(q, df);
    let i_sq = i_squared(q, df);
    let tau_sq = dsl_tau_squared(&weights, q, df);

    if random_effects && tau_sq > 0.0 {
        // Refit with DerSimonian–Laird adjusted weights.
        let re_weights: Vec<f64> = se_outcome
            .iter()
            .map(|s| 1.0 / (s * s + tau_sq))
            .collect();
        let re_fit = wls(&[beta_exposure], beta_outcome, &re_weights, false)?;
        Ok(IvwResult {
            estimate: re_fit.coefficients[0],
            se: re_fit.std_errors[0],
            p_value: re_fit.p_values[0],
            q_statistic: q,
            q_p_value,
            i_squared: i_sq,
            tau_squared: tau_sq,
            n_snps: k,
        })
    } else {
        Ok(IvwResult {
            estimate: fit.coefficients[0],
            se: fit.std_errors[0],
            p_value: fit.p_values[0],
            q_statistic: q,
            q_p_value,
            i_squared: i_sq,
            tau_squared: tau_sq,
            n_snps: k,
        })
    }
}

/// MR-Egger Mendelian randomization.
///
/// WLS regression of `beta_outcome` on `beta_exposure` with an intercept
/// (estimating average directional pleiotropy) and inverse-variance weights
/// `1/SE_outcome²`.
///
/// Heterogeneity statistics use `k − 2` degrees of freedom (intercept + slope).
pub fn mr_egger(
    beta_exposure: &[f64],
    beta_outcome: &[f64],
    se_outcome: &[f64],
) -> Result<EggerResult> {
    validate_mr_input(beta_exposure, beta_outcome, se_outcome)?;
    let k = beta_exposure.len();
    let weights: Vec<f64> = se_outcome.iter().map(|s| 1.0 / (s * s)).collect();

    let fit = wls(&[beta_exposure], beta_outcome, &weights, true)?;
    // coefficients[0] = intercept, coefficients[1] = slope.
    let q = fit.rss;
    let df = (k - 2) as f64;
    let q_p_value = chi2_sf(q, df);
    let i_sq = i_squared(q, df);
    let tau_sq = dsl_tau_squared(&weights, q, df);

    Ok(EggerResult {
        intercept: fit.coefficients[0],
        intercept_se: fit.std_errors[0],
        intercept_p_value: fit.p_values[0],
        slope: fit.coefficients[1],
        slope_se: fit.std_errors[1],
        slope_p_value: fit.p_values[1],
        q_statistic: q,
        q_p_value,
        i_squared: i_sq,
        tau_squared: tau_sq,
        n_snps: k,
    })
}

// ---------------------------------------------------------------------------
// Heterogeneity helpers
// ---------------------------------------------------------------------------

/// DerSimonian–Laird τ².
///
/// `c = Σwᵢ − Σwᵢ² / Σwᵢ`, then `τ² = (Q − df) / c`, clamped to 0.
fn dsl_tau_squared(weights: &[f64], q: f64, df: f64) -> f64 {
    let w_sum = compensated_sum(weights.iter().copied());
    let w_sq_sum = compensated_sum(weights.iter().map(|w| w * w));
    let c = w_sum - w_sq_sum / w_sum;
    if c > 0.0 && q > df {
        (q - df) / c
    } else {
        0.0
    }
}

/// I² as a percentage: `max(0, (Q − df) / Q) × 100`.
fn i_squared(q: f64, df: f64) -> f64 {
    if q > df {
        (q - df) / q * 100.0
    } else {
        0.0
    }
}

/// χ² survival function: `1 − F(Q; df)`. Returns 1.0 when Q ≤ 0.
fn chi2_sf(q: f64, df: f64) -> f64 {
    if q > 0.0 && df > 0.0 {
        ChiSquared::new(df).sf(q)
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

/// Validate common MR input: three slices of equal length, SEs > 0.
fn validate_mr_input(
    beta_exposure: &[f64],
    beta_outcome: &[f64],
    se_outcome: &[f64],
) -> Result<()> {
    let k = beta_exposure.len();
    if k == 0 {
        return Err(StatError::EmptyInput);
    }
    if beta_outcome.len() != k {
        return Err(StatError::LengthMismatch {
            a: k,
            b: beta_outcome.len(),
        });
    }
    if se_outcome.len() != k {
        return Err(StatError::LengthMismatch {
            a: k,
            b: se_outcome.len(),
        });
    }
    if se_outcome.iter().any(|s| *s <= 0.0 || s.is_nan()) {
        return Err(StatError::InvalidInput(
            "SE must be positive and finite".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    // ---- IVW tests ----

    #[test]
    fn ivw_perfect_ratio() {
        // All β_GY/β_GX = 1.5, equal SEs → exact IVW = 1.5, Q = 0.
        let bx = [0.1_f64, 0.2, 0.5];
        let by = [0.15_f64, 0.30, 0.75];
        let se = [0.05_f64, 0.05, 0.05];
        let r = ivw(&bx, &by, &se, false).unwrap();
        assert!(approx_eq(r.estimate, 1.5, 1e-9));
        assert!(r.q_statistic < 1e-20);
        assert!(r.i_squared < 1e-10);
        assert!(r.tau_squared < 1e-20);
        assert!(r.q_p_value > 0.99);
        assert_eq!(r.n_snps, 3);
    }

    #[test]
    fn ivw_known_heterogeneity() {
        // β̂ = Σw·x·y / Σw·x² with w=[400,100,400]: 132/44 = 3.0.
        // ŷ = 3·x → residuals [-0.05, -0.20, 0.05].
        // Q = 400·0.0025 + 100·0.04 + 400·0.0025 = 6.0.
        // I² = (6−2)/6·100 = 66.667%. Q_p = e^{-3} ≈ 0.0498 (χ² df=2).
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.25_f64, 0.40, 0.95];
        let se = [0.05_f64, 0.10, 0.05];
        let r = ivw(&bx, &by, &se, false).unwrap();
        assert!(approx_eq(r.estimate, 3.0, 1e-9));
        assert!(approx_eq(r.q_statistic, 6.0, 1e-9));
        assert!(approx_eq(r.i_squared, 66.6667, 1e-3));
        // χ²(6, df=2): P = gammq(1, 3) = e^{-3} ≈ 0.0498.
        assert!(approx_eq(r.q_p_value, 0.049787, 1e-4));
        assert_eq!(r.n_snps, 3);
    }

    #[test]
    fn ivw_tau_squared() {
        // Same data as ivw_known_heterogeneity.
        // w_sum = 900, w_sq_sum = 330000, c = 900 − 330000/900 = 533.33.
        // τ² = (6−2)/533.33 = 0.0075.
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.25_f64, 0.40, 0.95];
        let se = [0.05_f64, 0.10, 0.05];
        let r = ivw(&bx, &by, &se, false).unwrap();
        assert!(approx_eq(r.tau_squared, 0.0075, 1e-4));
    }

    #[test]
    fn ivw_random_effects_adjusts() {
        // When τ² > 0, RE refit should change estimate/SE from FE.
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.25_f64, 0.40, 0.95];
        let se = [0.05_f64, 0.10, 0.05];
        let fe = ivw(&bx, &by, &se, false).unwrap();
        let re = ivw(&bx, &by, &se, true).unwrap();
        assert!(fe.tau_squared > 0.0);
        // At minimum the SEs differ (RE weights are not 1/SE²).
        assert!(re.estimate != fe.estimate || re.se != fe.se);
        // Q, I², τ² come from the fixed-effect stage — must match.
        assert_eq!(fe.q_statistic, re.q_statistic);
        assert_eq!(fe.i_squared, re.i_squared);
        assert_eq!(fe.tau_squared, re.tau_squared);
    }

    #[test]
    fn ivw_random_effects_no_change_when_no_heterogeneity() {
        // Perfect ratio → τ² = 0 → RE falls back to FE.
        let bx = [0.1_f64, 0.2, 0.5];
        let by = [0.15_f64, 0.30, 0.75];
        let se = [0.05_f64, 0.05, 0.05];
        let fe = ivw(&bx, &by, &se, false).unwrap();
        let re = ivw(&bx, &by, &se, true).unwrap();
        assert_eq!(fe.estimate, re.estimate);
        assert_eq!(fe.se, re.se);
    }

    // ---- MR-Egger tests ----

    #[test]
    fn egger_perfect_fit_coefficients() {
        // y = 0.5 + x, equal SEs → exact intercept=0.5, slope=1.0.
        let bx = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let by = [1.5_f64, 2.5, 3.5, 4.5, 5.5];
        let se = [0.1_f64; 5];
        let r = mr_egger(&bx, &by, &se).unwrap();
        assert!(approx_eq(r.intercept, 0.5, 1e-9));
        assert!(approx_eq(r.slope, 1.0, 1e-9));
        assert!(r.q_statistic < 1e-9);
        assert_eq!(r.n_snps, 5);
    }

    #[test]
    fn egger_no_pleiotropy() {
        // y = 2x exactly → intercept = 0, slope = 2.
        let bx = [1.0_f64, 2.0, 3.0, 4.0];
        let by = [2.0_f64, 4.0, 6.0, 8.0];
        let se = [0.1_f64; 4];
        let r = mr_egger(&bx, &by, &se).unwrap();
        assert!(approx_eq(r.intercept, 0.0, 1e-9));
        assert!(approx_eq(r.slope, 2.0, 1e-9));
    }

    #[test]
    fn egger_with_noise() {
        // y ≈ 0.5 + x + small noise → finite p-values.
        let bx = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let by = [1.6_f64, 2.7, 3.3, 4.6, 5.4];
        let se = [0.1_f64; 5];
        let r = mr_egger(&bx, &by, &se).unwrap();
        assert!(r.intercept_p_value.is_finite());
        assert!(r.slope_p_value.is_finite());
        assert!((0.0..=1.0).contains(&r.intercept_p_value));
        assert!((0.0..=1.0).contains(&r.slope_p_value));
        // Q with k-2=3 df should be positive (there is noise).
        assert!(r.q_statistic > 0.0);
        assert!((0.0..=1.0).contains(&r.q_p_value));
    }

    #[test]
    fn egger_q_uses_k_minus_2_df() {
        // 3 SNPs → Egger uses df=1 for Q.
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.15_f64, 0.50, 0.85];
        let se = [0.05_f64; 3];
        let r = mr_egger(&bx, &by, &se).unwrap();
        assert_eq!(r.n_snps, 3);
        assert!(r.q_p_value >= 0.0 && r.q_p_value <= 1.0);
        // τ² clamped to 0 when Q < df (df=1 here).
        if r.q_statistic < 1.0 {
            assert_eq!(r.tau_squared, 0.0);
        }
    }

    // ---- Cross-method consistency ----

    #[test]
    fn ivw_egger_agree_when_no_pleiotropy() {
        // y = 2x, equal SEs → IVW slope ≈ Egger slope, Egger intercept ≈ 0.
        let bx = [0.5_f64, 1.0, 1.5];
        let by = [1.0_f64, 2.0, 3.0];
        let se = [0.1_f64; 3];
        let ivw_r = ivw(&bx, &by, &se, false).unwrap();
        let egger_r = mr_egger(&bx, &by, &se).unwrap();
        // With equal weights and no intercept, through-origin and with-intercept
        // slopes agree when the true intercept is zero.
        assert!(approx_eq(ivw_r.estimate, egger_r.slope, 1e-9));
        assert!(approx_eq(egger_r.intercept, 0.0, 1e-6));
    }

    // ---- Validation tests ----

    #[test]
    fn validation_errors() {
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.15_f64, 0.30, 0.45];
        let se = [0.05_f64, 0.05, 0.05];

        // Empty input.
        assert!(matches!(ivw(&[], &[], &[], false), Err(StatError::EmptyInput)));
        assert!(matches!(mr_egger(&[], &[], &[]), Err(StatError::EmptyInput)));

        // Length mismatch.
        assert!(matches!(
            ivw(&bx, &by[..2], &se, false),
            Err(StatError::LengthMismatch { .. })
        ));
        assert!(matches!(
            mr_egger(&bx, &by, &se[..2]),
            Err(StatError::LengthMismatch { .. })
        ));

        // SE ≤ 0.
        let bad_se = [0.05_f64, 0.0, 0.05];
        assert!(matches!(
            ivw(&bx, &by, &bad_se, false),
            Err(StatError::InvalidInput(_))
        ));

        // Egger needs ≥ 3 SNPs (2 params → n ≥ 3).
        assert!(matches!(
            mr_egger(&[0.1_f64], &[0.15_f64], &[0.05_f64]),
            Err(StatError::InsufficientData { .. })
        ));

        // IVW needs ≥ 2 SNPs (1 param → n ≥ 2).
        assert!(matches!(
            ivw(&[0.1_f64], &[0.15_f64], &[0.05_f64], false),
            Err(StatError::InsufficientData { .. })
        ));
    }

    #[test]
    fn ivw_p_value_finite() {
        // Heterogeneous data → σ̂² > 0 → finite p-value.
        let bx = [0.1_f64, 0.2, 0.3];
        let by = [0.25_f64, 0.40, 0.95];
        let se = [0.05_f64, 0.10, 0.05];
        let r = ivw(&bx, &by, &se, false).unwrap();
        assert!(r.p_value.is_finite());
        assert!((0.0..=1.0).contains(&r.p_value));
        assert!(r.se > 0.0);
    }
}
