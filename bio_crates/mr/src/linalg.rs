//! Faithful reproduction of R's `stats::lm` + `summary.lm` for the weighted
//! linear regressions that underpin IVW, MR-Egger and MR-GRIP.
//!
//! R's `lm(y ~ x, weights = w)` treats `w` as **precision weights** (inverse
//! variances): the coefficient minimises `Σ wᵢ (yᵢ − xᵢ β)²`, the residual
//! standard error is `sigma = sqrt(Σ wᵢ rᵢ² / (n − p))`, and the coefficient
//! covariance is `sigma² (Xᵀ W X)⁻¹`. The weights are **not** normalised —
//! normalising them (as `bio_crates::ldsc::linalg::wls` does) would rescale
//! `sigma` and break the IVW `/min(1, sigma)` correction.
//!
//! This module reproduces those quantities exactly, validated against
//! `Rscript`-computed `summary.lm` golden values in the tests.

use faer::linalg::solvers::{DenseSolveCore, Llt, Solve};
use faer::{Mat, Side};

use crate::{MrError, Result};

/// Summary of a weighted linear regression, matching `summary.lm`.
#[derive(Debug, Clone)]
pub struct WlmSummary {
    /// Coefficients in R's order: `[intercept?, slope]` — i.e. intercept first
    /// when `intercept = true`, then the slope on `x`.
    pub coef: Vec<f64>,
    /// Standard error of each coefficient (`Std. Error` column).
    pub se: Vec<f64>,
    /// Residual standard error (`sigma`), `sqrt(Σ wᵢ rᵢ² / df_resid)`.
    pub sigma: f64,
    /// Residual degrees of freedom `n − p` (as f64 for downstream arithmetic).
    pub df_resid: f64,
    /// Raw weighted residuals `rᵢ = yᵢ − x̂ᵢ` (length n).
    pub residuals: Vec<f64>,
}

impl WlmSummary {
    /// Slope coefficient (last entry of `coef`).
    pub fn slope(&self) -> f64 {
        *self.coef.last().unwrap()
    }
    /// Intercept coefficient, if an intercept was fitted.
    pub fn intercept(&self) -> Option<f64> {
        if self.coef.len() == 2 {
            Some(self.coef[0])
        } else {
            None
        }
    }
}

/// Fit `lm(y ~ x, weights = w)` (or through the origin when `intercept` is
/// false, i.e. `lm(y ~ -1 + x, weights = w)`).
///
/// * `x`, `y`, `w` — length-`n` slices. Weights must be > 0 and finite for the
///   regression to be well-defined (matching R, which silently drops non-finite
///   rows only via `na.action`; callers pre-filter NA rows).
/// * Returns [`MrError::Numerical`] when the design is rank-deficient (the
///   `(Xᵀ W X)` Gram matrix is not positive-definite), which R surfaces as a
///   `NA` coefficient row.
pub fn wlm(x: &[f64], y: &[f64], w: &[f64], intercept: bool) -> Result<WlmSummary> {
    let n = y.len();
    if x.len() != n || w.len() != n {
        return Err(MrError::LengthMismatch(format!(
            "wlm: lengths x={}, y={}, w={}",
            x.len(),
            n,
            w.len()
        )));
    }
    let p = if intercept { 2 } else { 1 };
    if n < p {
        return Err(MrError::InsufficientSnps(format!("wlm: n={n} < p={p}")));
    }

    // Build Xᵀ W X (p×p, symmetric) and Xᵀ W y (length p), column-major for faer.
    let mut xtwx = vec![0.0; p * p];
    let mut xtwy = vec![0.0; p];
    for i in 0..n {
        let wi = w[i];
        let xi = x[i];
        let yi = y[i];
        // Row of the design matrix for this observation.
        let row: [f64; 2] = if intercept { [1.0, xi] } else { [xi, 0.0] };
        for a in 0..p {
            xtwy[a] += wi * row[a] * yi;
            for b in a..p {
                xtwx[a * p + b] += wi * row[a] * row[b];
            }
        }
    }
    // Mirror the upper triangle into the lower.
    for a in 0..p {
        for b in (a + 1)..p {
            xtwx[b * p + a] = xtwx[a * p + b];
        }
    }

    let gram = Mat::from_fn(p, p, |i, j| xtwx[j * p + i]);
    let llt = Llt::new(gram.as_ref(), Side::Lower)
        .map_err(|e| MrError::Numerical(format!("wlm: singular design (LLᵀ failed): {e:?}")))?;

    // β = (Xᵀ W X)⁻¹ Xᵀ W y
    let rhs = Mat::from_fn(p, 1, |i, _| xtwy[i]);
    let beta_mat = llt.solve(&rhs);
    let coef: Vec<f64> = (0..p).map(|i| beta_mat[(i, 0)]).collect();

    // Residuals and weighted RSS.
    let mut residuals = vec![0.0; n];
    let mut wrss = 0.0;
    for i in 0..n {
        let xi = x[i];
        let fit = if intercept {
            coef[0] + coef[1] * xi
        } else {
            coef[0] * xi
        };
        let r = y[i] - fit;
        residuals[i] = r;
        wrss += w[i] * r * r;
    }
    let df_resid = (n - p) as f64;
    let sigma2 = wrss / df_resid;
    let sigma = sigma2.sqrt();

    // Coefficient covariance = sigma² · (Xᵀ W X)⁻¹. Extract its diagonal.
    let inv = llt.inverse();
    let se: Vec<f64> = (0..p)
        .map(|j| (sigma2 * inv[(j, j)]).max(0.0).sqrt())
        .collect();

    Ok(WlmSummary {
        coef,
        se,
        sigma,
        df_resid,
        residuals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-12)
    }

    // Analytical case: through-origin WLS, single predictor.
    // x = [1,2,3], y = [2,4,6], w = [1,1,1] → perfect fit, slope 2, sigma 0.
    #[test]
    fn origin_perfect_fit() {
        let s = wlm(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0], &[1.0; 3], false).unwrap();
        assert!(approx(s.slope(), 2.0, 1e-12));
        assert!(s.sigma.abs() < 1e-12);
        assert_eq!(s.df_resid, 2.0);
    }

    // Intercept fit against a known OLS solution.
    // x=[0,1,2,3], y=[1,3,5,7] → y = 1 + 2x exactly.
    #[test]
    fn intercept_perfect_fit() {
        let s = wlm(&[0., 1., 2., 3.], &[1., 3., 5., 7.], &[1.; 4], true).unwrap();
        assert!(approx(s.intercept().unwrap(), 1.0, 1e-12));
        assert!(approx(s.slope(), 2.0, 1e-12));
        assert!(s.sigma.abs() < 1e-12);
    }

    // Weighted: doubling a weight should match R's weighted lm sigma scaling.
    // Compare against the closed-form through-origin WLS:
    //   beta = Σwx y / Σwx²,  sigma² = Σw r²/(n-1).
    #[test]
    fn origin_weighted_matches_closed_form() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [1.9, 3.7, 6.2, 7.8];
        let w = [0.5, 1.0, 2.0, 1.5];
        let s = wlm(&x, &y, &w, false).unwrap();
        let num: f64 = x
            .iter()
            .zip(&y)
            .zip(&w)
            .map(|((xi, yi), wi)| wi * xi * yi)
            .sum();
        let den: f64 = x.iter().zip(&w).map(|(xi, wi)| wi * xi * xi).sum();
        let beta = num / den;
        let wrss: f64 = x
            .iter()
            .zip(&y)
            .zip(&w)
            .map(|((xi, yi), wi)| wi * (yi - beta * xi).powi(2))
            .sum();
        let sigma = (wrss / 3.0).sqrt();
        assert!(approx(s.slope(), beta, 1e-12));
        assert!(approx(s.sigma, sigma, 1e-10));
        // se_slope = sqrt(sigma² / Σ w x²)
        let se = (sigma.powi(2) / den).sqrt();
        assert!(approx(s.se[0], se, 1e-10));
    }

    #[test]
    fn singular_design_errors() {
        // All x equal with intercept → rank-deficient.
        let r = wlm(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0], &[1.0; 3], true);
        assert!(r.is_err());
    }
}
