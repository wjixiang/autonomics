//! Unweighted descriptive statistics over `&[f64]`.
//!
//! Conventions:
//! - Empty input is an error ([`StatError::EmptyInput`]); a single value is
//!   valid where the math allows (e.g. `mean`, `min`, `max`) and an error
//!   where it does not (sample variance needs `n ≥ 2`).
//! - "Variance"/"standard deviation" default to the **sample** form (`n − 1`,
//!   unbiased). Population variants (`n`) are named `*_population`.
//! - Quantiles use **linear interpolation between order statistics** — R type 7
//!   / NumPy default — the most common convention.

use crate::error::{Result, StatError};
use crate::util::compensated_sum;

/// Sum of the values (compensated, to limit accumulation error).
pub fn sum(xs: &[f64]) -> Result<f64> {
    if xs.is_empty() {
        return Err(StatError::EmptyInput);
    }
    Ok(compensated_sum(xs.iter().copied()))
}

/// Arithmetic mean.
pub fn mean(xs: &[f64]) -> Result<f64> {
    if xs.is_empty() {
        return Err(StatError::EmptyInput);
    }
    Ok(compensated_sum(xs.iter().copied()) / xs.len() as f64)
}

/// Variance with a given delta-degrees-of-freedom (`ddof`). `ddof=1` → sample,
/// `ddof=0` → population.
fn variance_ddof(xs: &[f64], ddof: usize) -> Result<f64> {
    let n = xs.len();
    if n <= ddof {
        return Err(StatError::InsufficientData {
            min: ddof + 1,
            actual: n,
        });
    }
    let mu = compensated_sum(xs.iter().copied()) / n as f64;
    let ss = compensated_sum(xs.iter().map(|&x| {
        let d = x - mu;
        d * d
    }));
    Ok(ss / (n - ddof) as f64)
}

/// Sample variance (`n − 1` denominator, unbiased estimator).
pub fn variance(xs: &[f64]) -> Result<f64> {
    variance_ddof(xs, 1)
}

/// Population variance (`n` denominator).
pub fn variance_population(xs: &[f64]) -> Result<f64> {
    variance_ddof(xs, 0)
}

/// Sample standard deviation.
pub fn std_dev(xs: &[f64]) -> Result<f64> {
    Ok(variance(xs)?.sqrt())
}

/// Population standard deviation.
pub fn std_dev_population(xs: &[f64]) -> Result<f64> {
    Ok(variance_population(xs)?.sqrt())
}

fn covariance_ddof(xs: &[f64], ys: &[f64], ddof: usize) -> Result<f64> {
    let n = xs.len();
    if n != ys.len() {
        return Err(StatError::LengthMismatch { a: n, b: ys.len() });
    }
    if n <= ddof {
        return Err(StatError::InsufficientData {
            min: ddof + 1,
            actual: n,
        });
    }
    let mx = compensated_sum(xs.iter().copied()) / n as f64;
    let my = compensated_sum(ys.iter().copied()) / n as f64;
    let cov = compensated_sum(xs.iter().zip(ys).map(|(&x, &y)| (x - mx) * (y - my)));
    Ok(cov / (n - ddof) as f64)
}

/// Sample covariance (`n − 1`).
pub fn covariance(xs: &[f64], ys: &[f64]) -> Result<f64> {
    covariance_ddof(xs, ys, 1)
}

/// Population covariance (`n`).
pub fn covariance_population(xs: &[f64], ys: &[f64]) -> Result<f64> {
    covariance_ddof(xs, ys, 0)
}

/// Pearson correlation coefficient, `cov(x,y) / (σ_x · σ_y)`.
///
/// Returns an error if either variable has zero variance (constant input).
pub fn correlation(xs: &[f64], ys: &[f64]) -> Result<f64> {
    let n = xs.len();
    if n != ys.len() {
        return Err(StatError::LengthMismatch { a: n, b: ys.len() });
    }
    if n < 2 {
        return Err(StatError::InsufficientData { min: 2, actual: n });
    }
    let mx = compensated_sum(xs.iter().copied()) / n as f64;
    let my = compensated_sum(ys.iter().copied()) / n as f64;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (&x, &y) in xs.iter().zip(ys) {
        let dx = x - mx;
        let dy = y - my;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    let denom = (sxx * syy).sqrt();
    if denom == 0.0 {
        return Err(StatError::InvalidInput(
            "zero variance: at least one input is constant".to_string(),
        ));
    }
    Ok(sxy / denom)
}

/// Quantile of an already-sorted slice, with linear interpolation (R type 7).
///
/// `q` must lie in `[0, 1]`. `sorted` must be non-empty and ascending; the
/// caller is responsible for ordering. Use [`quantile`] for the unsorted form.
pub fn quantile_sorted(sorted: &[f64], q: f64) -> Result<f64> {
    if !(0.0..=1.0).contains(&q) {
        return Err(StatError::InvalidQuantile(q));
    }
    let n = sorted.len();
    if n == 0 {
        return Err(StatError::EmptyInput);
    }
    if n == 1 {
        return Ok(sorted[0]);
    }
    // R type 7: index = (n−1)·q, then linearly interpolate between neighbours.
    let index = (n as f64 - 1.0) * q;
    let lo = index.floor();
    let hi = index.ceil();
    let frac = index - lo;
    Ok(sorted[lo as usize] * (1.0 - frac) + sorted[hi as usize] * frac)
}

/// Quantile (linear interpolation, R type 7 / NumPy default).
///
/// Copies and sorts the input internally. For repeated quantile queries on the
/// same data, sort once and call [`quantile_sorted`].
pub fn quantile(xs: &[f64], q: f64) -> Result<f64> {
    if xs.is_empty() {
        return Err(StatError::EmptyInput);
    }
    let mut sorted = xs.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    quantile_sorted(&sorted, q)
}

/// Median (the 0.5 quantile).
pub fn median(xs: &[f64]) -> Result<f64> {
    quantile(xs, 0.5)
}

/// Adjusted Fisher–Pearson sample skewness, `G₁`.
///
/// Unbiased under normality; requires `n ≥ 3`. Equals 0 for symmetric data.
pub fn skewness(xs: &[f64]) -> Result<f64> {
    let n = xs.len();
    if n < 3 {
        return Err(StatError::InsufficientData { min: 3, actual: n });
    }
    let nf = n as f64;
    let mu = compensated_sum(xs.iter().copied()) / nf;
    let mut m2 = 0.0; // second central moment
    let mut m3 = 0.0; // third central moment
    for &x in xs {
        let d = x - mu;
        let d2 = d * d;
        m2 += d2;
        m3 += d2 * d;
    }
    m2 /= nf;
    m3 /= nf;
    if m2 == 0.0 {
        return Err(StatError::InvalidInput(
            "zero variance: input is constant".to_string(),
        ));
    }
    // G₁ = (√(n(n−1))/(n−2)) · (m3/m2^(3/2))
    let g1 = ((nf * (nf - 1.0)).sqrt() / (nf - 2.0)) * (m3 / m2.powf(1.5));
    Ok(g1)
}

/// Excess sample kurtosis, `G₂` (Fisher's adjusted form; normal → 0).
///
/// Requires `n ≥ 4`.
pub fn kurtosis(xs: &[f64]) -> Result<f64> {
    let n = xs.len();
    if n < 4 {
        return Err(StatError::InsufficientData { min: 4, actual: n });
    }
    let nf = n as f64;
    let mu = compensated_sum(xs.iter().copied()) / nf;
    let mut m2 = 0.0;
    let mut m4 = 0.0;
    for &x in xs {
        let d = x - mu;
        let d2 = d * d;
        m2 += d2;
        m4 += d2 * d2;
    }
    m2 /= nf;
    m4 /= nf;
    if m2 == 0.0 {
        return Err(StatError::InvalidInput(
            "zero variance: input is constant".to_string(),
        ));
    }
    // G₂ = [(n+1)(n−1) / ((n−2)(n−3))] · (m4/m2²) − [3(n−1)² / ((n−2)(n−3))]
    // (Westfall 2014 unbiased excess kurtosis; 0 for a normal sample.)
    let denom = (nf - 2.0) * (nf - 3.0);
    let g2 = (nf + 1.0) * (nf - 1.0) / denom * (m4 / (m2 * m2))
        - 3.0 * (nf - 1.0).powi(2) / denom;
    Ok(g2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn mean_sum_basic() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(sum(&xs).unwrap(), 15.0);
        assert_eq!(mean(&xs).unwrap(), 3.0);
    }

    #[test]
    fn empty_is_error() {
        let xs: [f64; 0] = [];
        assert!(matches!(mean(&xs), Err(StatError::EmptyInput)));
        assert!(matches!(sum(&xs), Err(StatError::EmptyInput)));
    }

    #[test]
    fn variance_sample_vs_population() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!(approx_eq(variance(&xs).unwrap(), 2.5, 1e-12));
        assert!(approx_eq(variance_population(&xs).unwrap(), 2.0, 1e-12));
        assert!(approx_eq(std_dev(&xs).unwrap(), 2.5_f64.sqrt(), 1e-12));
    }

    #[test]
    fn variance_single_value_errors() {
        // Sample variance needs n ≥ 2.
        assert!(matches!(
            variance(&[5.0]),
            Err(StatError::InsufficientData { .. })
        ));
        // Population variance of one value is 0.
        assert_eq!(variance_population(&[5.0]).unwrap(), 0.0);
    }

    #[test]
    fn covariance_and_correlation() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        let ys = [2.0, 4.0, 6.0, 8.0]; // perfect linear, slope 2
        // Σ(x−x̄)(y−ȳ) = 10 over n−1 = 3 → sample covariance 10/3.
        assert!(approx_eq(covariance(&xs, &ys).unwrap(), 10.0 / 3.0, 1e-12));
        assert!(approx_eq(correlation(&xs, &ys).unwrap(), 1.0, 1e-12));
        assert!(approx_eq(
            correlation(&xs, &[-1.0, -2.0, -3.0, -4.0]).unwrap(),
            -1.0,
            1e-12
        ));
    }

    #[test]
    fn correlation_length_mismatch() {
        assert!(matches!(
            correlation(&[1.0, 2.0], &[1.0]),
            Err(StatError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn correlation_constant_is_error() {
        assert!(matches!(
            correlation(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]),
            Err(StatError::InvalidInput(_))
        ));
    }

    #[test]
    fn quantile_type7_known_values() {
        // R: quantile(c(1,2,3,4,5), probs=0.25, type=7) == 2.0; 0.5 == 3; 0.75 == 4.
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile(&xs, 0.0).unwrap(), 1.0);
        assert_eq!(quantile(&xs, 0.25).unwrap(), 2.0);
        assert_eq!(quantile(&xs, 0.5).unwrap(), 3.0);
        assert_eq!(quantile(&xs, 0.75).unwrap(), 4.0);
        assert_eq!(quantile(&xs, 1.0).unwrap(), 5.0);
        assert_eq!(median(&xs).unwrap(), 3.0);
        // 0.5 quantile of 1..6 (n=6) interpolates between index 2 and 3 → 3.5.
        let ys = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(median(&ys).unwrap(), 3.5);
    }

    #[test]
    fn quantile_validation() {
        assert!(matches!(
            quantile(&[1.0, 2.0], -0.1),
            Err(StatError::InvalidQuantile(_))
        ));
        assert!(matches!(
            quantile(&[1.0, 2.0], 1.1),
            Err(StatError::InvalidQuantile(_))
        ));
    }

    #[test]
    fn skewness_symmetric_is_zero() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0]; // symmetric
        assert!(approx_eq(skewness(&xs).unwrap(), 0.0, 1e-9));
    }

    #[test]
    fn kurtosis_uniform_is_negative() {
        // Excess kurtosis of a continuous uniform ≈ −1.2; the sample-adjusted
        // G₂ of a long uniform sample approaches that.
        let xs: Vec<f64> = (1..=1000).map(|i| i as f64).collect();
        assert!(approx_eq(kurtosis(&xs).unwrap(), -1.2, 1e-2));
    }
}
