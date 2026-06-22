//! Layer 3 — regression: ordinary and weighted least squares.
//!
//! Every Mendelian-randomization estimator is, at its core, a weighted
//! regression of outcome effects on exposure effects (IVW: through the origin,
//! inverse-variance weights; Egger: with an intercept). This layer provides the
//! [`Regression`] fit those methods compose from.
//!
//! The fit solves the normal equations `(Xᵀ W X) β = Xᵀ W y` by Gaussian
//! elimination with partial pivoting, then derives standard errors from the
//! estimated residual variance `σ̂² = RSS / (n − p)` and `(Xᵀ W X)⁻¹`. p-values
//! use the Student-t distribution with `n − p` residual degrees of freedom,
//! matching the convention used by MR software (TwoSampleMR / RadialMR).
//!
//! Predictors are supplied as parallel columns: `&[&[f64]]`, each inner slice of
//! length `n`. `intercept = true` prepends a column of ones, so
//! `coefficients[0]` is the intercept.

use crate::distribution::{Distribution, StudentT};
use crate::error::{Result, StatError};
use crate::util::compensated_sum;

/// Result of an OLS or WLS fit.
#[derive(Debug, Clone, PartialEq)]
pub struct Regression {
    /// Estimated coefficients, length `n_params`. When fit with an intercept,
    /// index 0 is the intercept followed by one coefficient per predictor.
    pub coefficients: Vec<f64>,
    /// Standard error of each coefficient.
    pub std_errors: Vec<f64>,
    /// `t = coefficient / std_error` for each coefficient.
    pub t_stats: Vec<f64>,
    /// Two-sided p-value from Student-t with `df_residual` degrees of freedom.
    pub p_values: Vec<f64>,
    /// Fitted values `ŷ = Xβ`.
    pub fitted: Vec<f64>,
    /// Residuals `y − ŷ`.
    pub residuals: Vec<f64>,
    /// Weighted residual sum of squares `Σ wᵢ (yᵢ − ŷᵢ)²`.
    pub rss: f64,
    /// Weighted total sum of squares (about the weighted mean if an intercept
    /// was fit; otherwise uncentered `Σ wᵢ yᵢ²`).
    pub tss: f64,
    /// Coefficient of determination `1 − RSS/TSS`.
    pub r_squared: f64,
    /// Adjusted R².
    pub adj_r_squared: f64,
    /// Number of observations.
    pub n_obs: usize,
    /// Number of estimated parameters (predictors + intercept if fit).
    pub n_params: usize,
    /// Residual degrees of freedom `n_obs − n_params`.
    pub df_residual: usize,
}

/// Weighted least squares: regress `y` on `predictors` with observation
/// `weights`, optionally fitting an intercept.
///
/// Weights are interpreted as inverse-variance (reliability) weights: the
/// estimated residual variance `σ̂² = RSS / (n − p)` inflates with
/// heterogeneity, which is exactly the behaviour IVW needs for its random-effects
/// variant. For frequency weights the point estimates are unchanged but the
/// variance estimate differs — call [`ols`] if weights are all equal.
pub fn wls(predictors: &[&[f64]], y: &[f64], weights: &[f64], intercept: bool) -> Result<Regression> {
    fit(predictors, y, weights, intercept)
}

/// Ordinary least squares: [`wls`] with unit weights.
pub fn ols(predictors: &[&[f64]], y: &[f64], intercept: bool) -> Result<Regression> {
    let unit: Vec<f64> = vec![1.0; y.len()];
    fit(predictors, y, &unit, intercept)
}

fn fit(predictors: &[&[f64]], y: &[f64], weights: &[f64], intercept: bool) -> Result<Regression> {
    let n = y.len();
    if n == 0 {
        return Err(StatError::EmptyInput);
    }
    if weights.len() != n {
        return Err(StatError::LengthMismatch {
            a: n,
            b: weights.len(),
        });
    }
    if weights.iter().any(|&w| w < 0.0 || w.is_nan()) {
        return Err(StatError::InvalidWeights);
    }
    for (i, p) in predictors.iter().enumerate() {
        if p.len() != n {
            return Err(StatError::LengthMismatch { a: n, b: p.len() });
        }
        let _ = i;
    }

    // Build design columns: optional intercept (all ones) then each predictor.
    let mut cols: Vec<Vec<f64>> = Vec::with_capacity(predictors.len() + intercept as usize);
    if intercept {
        cols.push(vec![1.0; n]);
    }
    for p in predictors {
        cols.push(p.to_vec());
    }
    let p = cols.len();
    let df_residual = n.checked_sub(p).filter(|&d| d > 0).ok_or_else(|| {
        StatError::InsufficientData {
            min: p + 1,
            actual: n,
        }
    })?;

    // Normal equations: XtWX (p×p) and XtWy (p).
    let mut xtx = vec![vec![0.0; p]; p];
    let mut xty = vec![0.0; p];
    for i in 0..p {
        for j in i..p {
            let s = compensated_sum(
                (0..n).map(|k| weights[k] * cols[i][k] * cols[j][k]),
            );
            xtx[i][j] = s;
            xtx[j][i] = s;
        }
        xty[i] = compensated_sum((0..n).map(|k| weights[k] * cols[i][k] * y[k]));
    }

    let xtx_inv = invert(&xtx)?;
    // β = (XtWX)^−1 · XtWy
    let mut coefficients = vec![0.0; p];
    for i in 0..p {
        coefficients[i] = compensated_sum((0..p).map(|j| xtx_inv[i][j] * xty[j]));
    }

    // Fitted values and residuals.
    let mut fitted = vec![0.0; n];
    for k in 0..n {
        fitted[k] = compensated_sum((0..p).map(|j| coefficients[j] * cols[j][k]));
    }
    let residuals: Vec<f64> = (0..n).map(|k| y[k] - fitted[k]).collect();

    let rss = compensated_sum((0..n).map(|k| weights[k] * residuals[k] * residuals[k]));

    // Total SS about the weighted mean if an intercept was fit, else uncentered.
    let (tss, mean) = if intercept {
        let wsum = compensated_sum(weights.iter().copied());
        let wy = compensated_sum((0..n).map(|k| weights[k] * y[k]));
        let mean = wy / wsum;
        let tss = compensated_sum((0..n).map(|k| weights[k] * (y[k] - mean) * (y[k] - mean)));
        (tss, Some(mean))
    } else {
        let tss = compensated_sum((0..n).map(|k| weights[k] * y[k] * y[k]));
        (tss, None)
    };
    let _ = mean;

    let r_squared = if tss > 0.0 {
        1.0 - rss / tss
    } else {
        f64::NAN
    };
    let adj_r_squared =
        1.0 - (1.0 - r_squared) * (n as f64 - 1.0) / df_residual as f64;

    // Variance estimate and coefficient standard errors.
    let sigma2 = rss / df_residual as f64;
    let t_dist = StudentT::new(df_residual as f64);
    let std_errors: Vec<f64> = (0..p)
        .map(|i| (sigma2 * xtx_inv[i][i]).max(0.0).sqrt())
        .collect();
    let t_stats: Vec<f64> = (0..p)
        .map(|i| {
            if std_errors[i] > 0.0 {
                coefficients[i] / std_errors[i]
            } else {
                f64::NAN
            }
        })
        .collect();
    let p_values: Vec<f64> = (0..p)
        .map(|i| {
            let t = t_stats[i].abs();
            if t.is_finite() {
                2.0 * t_dist.sf(t)
            } else {
                f64::NAN
            }
        })
        .collect();

    Ok(Regression {
        coefficients,
        std_errors,
        t_stats,
        p_values,
        fitted,
        residuals,
        rss,
        tss,
        r_squared,
        adj_r_squared,
        n_obs: n,
        n_params: p,
        df_residual,
    })
}

/// Invert a square matrix by Gauss-Jordan elimination with partial pivoting.
/// Returns [`StatError::SingularMatrix`] if the matrix is not invertible.
fn invert(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = a.len();
    // Augmented [A | I].
    let mut m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let mut row = a[i].clone();
            row.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            row
        })
        .collect();
    const TINY: f64 = 1.0e-300;

    for k in 0..n {
        // Partial pivot.
        let mut piv = k;
        for r in (k + 1)..n {
            if m[r][k].abs() > m[piv][k].abs() {
                piv = r;
            }
        }
        if m[piv][k].abs() <= TINY {
            return Err(StatError::SingularMatrix);
        }
        if piv != k {
            m.swap(k, piv);
        }
        let diag = m[k][k];
        for elem in m[k].iter_mut() {
            *elem /= diag;
        }
        // Eliminate the column from all other rows.
        for r in 0..n {
            if r == k {
                continue;
            }
            let factor = m[r][k];
            if factor == 0.0 {
                continue;
            }
            // Borrow two distinct rows without overlapping aliasing.
            let (row_r, row_k) = if r < k {
                let (lo, hi) = m.split_at_mut(k);
                (&mut lo[r], &hi[0])
            } else {
                let (lo, hi) = m.split_at_mut(r);
                (&mut hi[0], &lo[k])
            };
            for (rr, kk) in row_r.iter_mut().zip(row_k.iter()) {
                *rr -= factor * kk;
            }
        }
    }
    let inv: Vec<Vec<f64>> = m.into_iter().map(|row| row[n..].to_vec()).collect();
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn simple_linear_regression_closed_form() {
        // x = [1,2,3,4,5], y = [2,4,5,4,5]: slope = cov/var = 1.5/2.5 = 0.6,
        // intercept = 4 − 0.6·3 = 2.2, R² = 0.6.
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 4.0, 5.0, 4.0, 5.0];
        let r = ols(&[&x[..]], &y, true).unwrap();
        assert!(approx_eq(r.coefficients[0], 2.2, 1e-9)); // intercept
        assert!(approx_eq(r.coefficients[1], 0.6, 1e-9)); // slope
        assert!(approx_eq(r.r_squared, 0.6, 1e-9));
        assert!(approx_eq(r.tss, 6.0, 1e-9)); // Σ(y−ȳ)² = 6
    }

    #[test]
    fn through_origin_fit() {
        // y = 2x exactly, no intercept: β = Σxy/Σx² = 28/14 = 2.
        let x = [1.0, 2.0, 3.0];
        let y = [2.0, 4.0, 6.0];
        let r = ols(&[&x[..]], &y, false).unwrap();
        assert_eq!(r.n_params, 1);
        assert!(approx_eq(r.coefficients[0], 2.0, 1e-9));
    }

    #[test]
    fn wls_unit_weights_equals_ols() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [3.0, 5.0, 7.0, 9.0, 11.0];
        let w = [1.0; 5];
        let a = ols(&[&x[..]], &y, true).unwrap();
        let b = wls(&[&x[..]], &y, &w, true).unwrap();
        assert_eq!(a.coefficients, b.coefficients);
        assert!(approx_eq(a.r_squared, b.r_squared, 1e-12));
    }

    #[test]
    fn perfect_fit_zero_rss() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [3.0, 5.0, 7.0, 9.0]; // y = 2x + 1
        let r = ols(&[&x[..]], &y, true).unwrap();
        assert!(approx_eq(r.coefficients[0], 1.0, 1e-9));
        assert!(approx_eq(r.coefficients[1], 2.0, 1e-9));
        assert!(r.rss.abs() < 1e-9);
        assert!(approx_eq(r.r_squared, 1.0, 1e-9));
    }

    #[test]
    fn multiple_regression_recovers_coefficients() {
        // y = 1 + 2·x1 + 3·x2 on a few points.
        let x1 = [0.0, 1.0, 2.0, 3.0, 4.0];
        let x2 = [0.0, 1.0, 0.0, 1.0, 0.0];
        let y: Vec<f64> = (0..5).map(|i| 1.0 + 2.0 * x1[i] + 3.0 * x2[i]).collect();
        let r = ols(&[&x1[..], &x2[..]], &y, true).unwrap();
        assert!(approx_eq(r.coefficients[0], 1.0, 1e-9));
        assert!(approx_eq(r.coefficients[1], 2.0, 1e-9));
        assert!(approx_eq(r.coefficients[2], 3.0, 1e-9));
    }

    #[test]
    fn singular_design_errors() {
        // Two identical predictor columns → perfect collinearity.
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [2.0, 4.0, 6.0, 8.0];
        let res = ols(&[&x[..], &x[..]], &y, true);
        assert!(matches!(res, Err(StatError::SingularMatrix)));
    }

    #[test]
    fn insufficient_observations_errors() {
        // n_obs = 2 but fitting intercept + slope needs ≥ 3.
        let x = [1.0, 2.0];
        let y = [3.0, 5.0];
        let res = ols(&[&x[..]], &y, true);
        assert!(matches!(res, Err(StatError::InsufficientData { .. })));
    }

    #[test]
    fn p_values_finite_and_in_range() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 4.0, 5.0, 4.0, 5.0];
        let r = ols(&[&x[..]], &y, true).unwrap();
        for &p in &r.p_values {
            assert!(p.is_finite() && (0.0..=1.0).contains(&p));
        }
        // t = coef / se.
        for i in 0..r.n_params {
            assert!(approx_eq(
                r.t_stats[i],
                r.coefficients[i] / r.std_errors[i],
                1e-9
            ));
        }
    }

    #[test]
    fn validation_errors() {
        let x = [1.0, 2.0, 3.0];
        let y = [1.0, 2.0, 3.0];
        // Length mismatch in weights.
        assert!(matches!(
            wls(&[&x[..]], &y, &[1.0, 1.0], false),
            Err(StatError::LengthMismatch { .. })
        ));
        // Negative weight.
        assert!(matches!(
            wls(&[&x[..]], &y, &[1.0, -1.0, 1.0], false),
            Err(StatError::InvalidWeights)
        ));
    }
}
