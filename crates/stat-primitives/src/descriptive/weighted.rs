//! Weighted descriptive statistics.
//!
//! Weights are interpreted as **frequency/reliability weights**: non-negative,
//! and not all zero. The weighted mean is `Σ wᵢxᵢ / Σ wᵢ`. Sample statistics
//! use a `Σw − 1` denominator (the frequency-weight analogue of `n − 1`);
//! population variants use `Σw`.
//!
//! This is the descriptive (unstructured) layer. The meta-analysis layer will
//! introduce its own inverse-variance-weighted estimators on top of these.

use crate::error::{Result, StatError};
use crate::util::compensated_sum;

/// Validate weight slices: equal length, all non-negative, positive total.
fn validate_weights(values: &[f64], weights: &[f64]) -> Result<f64> {
    if values.len() != weights.len() {
        return Err(StatError::LengthMismatch {
            a: values.len(),
            b: weights.len(),
        });
    }
    if values.is_empty() {
        return Err(StatError::EmptyInput);
    }
    if weights.iter().any(|&w| w < 0.0 || w.is_nan()) {
        return Err(StatError::InvalidWeights);
    }
    let wsum = compensated_sum(weights.iter().copied());
    if !wsum.is_finite() || wsum <= 0.0 {
        return Err(StatError::InvalidWeights);
    }
    Ok(wsum)
}

/// Weighted mean, `Σ wᵢxᵢ / Σ wᵢ`.
pub fn weighted_mean(values: &[f64], weights: &[f64]) -> Result<f64> {
    let wsum = validate_weights(values, weights)?;
    let num = compensated_sum(values.iter().zip(weights).map(|(&x, &w)| x * w));
    Ok(num / wsum)
}

/// Weighted sample variance (`Σw − 1` denominator).
pub fn weighted_variance(values: &[f64], weights: &[f64]) -> Result<f64> {
    let wsum = validate_weights(values, weights)?;
    if wsum <= 1.0 {
        return Err(StatError::InsufficientData {
            min: 2,
            actual: 1, // total weight too small for the sample form
        });
    }
    let mu = weighted_mean(values, weights)?;
    let num = compensated_sum(
        values
            .iter()
            .zip(weights)
            .map(|(&x, &w)| w * (x - mu) * (x - mu)),
    );
    Ok(num / (wsum - 1.0))
}

/// Weighted population variance (`Σw` denominator).
pub fn weighted_variance_population(values: &[f64], weights: &[f64]) -> Result<f64> {
    let wsum = validate_weights(values, weights)?;
    let mu = weighted_mean(values, weights)?;
    let num = compensated_sum(
        values
            .iter()
            .zip(weights)
            .map(|(&x, &w)| w * (x - mu) * (x - mu)),
    );
    Ok(num / wsum)
}

/// Weighted sample covariance (`Σw − 1`).
pub fn weighted_covariance(xs: &[f64], ys: &[f64], weights: &[f64]) -> Result<f64> {
    let wsum = validate_weights(xs, weights)?;
    if ys.len() != xs.len() {
        return Err(StatError::LengthMismatch {
            a: xs.len(),
            b: ys.len(),
        });
    }
    if wsum <= 1.0 {
        return Err(StatError::InsufficientData { min: 2, actual: 1 });
    }
    let mx = weighted_mean(xs, weights)?;
    let my = weighted_mean(ys, weights)?;
    let num = compensated_sum(
        xs.iter()
            .zip(ys)
            .zip(weights)
            .map(|((&x, &y), &w)| w * (x - mx) * (y - my)),
    );
    Ok(num / (wsum - 1.0))
}

/// Weighted Pearson correlation.
///
/// Errors if either weighted variable has zero variance.
pub fn weighted_correlation(xs: &[f64], ys: &[f64], weights: &[f64]) -> Result<f64> {
    let wsum = validate_weights(xs, weights)?;
    if ys.len() != xs.len() {
        return Err(StatError::LengthMismatch {
            a: xs.len(),
            b: ys.len(),
        });
    }
    let mx = weighted_mean(xs, weights)?;
    let my = weighted_mean(ys, weights)?;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for ((&x, &y), &w) in xs.iter().zip(ys).zip(weights) {
        let dx = x - mx;
        let dy = y - my;
        sxy += w * dx * dy;
        sxx += w * dx * dx;
        syy += w * dy * dy;
    }
    let denom = (sxx * syy).sqrt();
    if denom == 0.0 {
        return Err(StatError::InvalidInput(
            "zero weighted variance: at least one input is constant".to_string(),
        ));
    }
    let _ = wsum; // denominator cancels in the ratio; included for clarity.
    Ok(sxy / denom)
}

/// Weighted quantile via linear interpolation over cumulative weights.
///
/// Finds the value at which the cumulative weight reaches `q · Σw`, linearly
/// interpolating between the two bracketing sorted observations.
pub fn weighted_quantile(values: &[f64], weights: &[f64], q: f64) -> Result<f64> {
    if !(0.0..=1.0).contains(&q) {
        return Err(StatError::InvalidQuantile(q));
    }
    let wsum = validate_weights(values, weights)?;
    // Sort by value, carrying the weights.
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let target = q * wsum;

    // Walk the cumulative-weight curve, interpolating between consecutive
    // distinct sorted values when `target` lands inside a weight bin.
    let mut prev_value = values[idx[0]];
    let mut prev_cum = 0.0;
    for &i in &idx {
        let cum = prev_cum + weights[i];
        if target <= cum {
            if cum == prev_cum {
                return Ok(values[i]);
            }
            let frac = (target - prev_cum) / (cum - prev_cum);
            return Ok(prev_value + frac * (values[i] - prev_value));
        }
        prev_value = values[i];
        prev_cum = cum;
    }
    // target exceeds the total weight (rounding) → largest value.
    Ok(values[idx[idx.len() - 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn weighted_mean_basic() {
        let v = [1.0, 2.0, 3.0, 4.0];
        let w = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(weighted_mean(&v, &w).unwrap(), 2.5);
        // Weight heavily toward the last value.
        let w2 = [1.0, 1.0, 1.0, 7.0];
        assert!(approx_eq(weighted_mean(&v, &w2).unwrap(), 3.4, 1e-12));
    }

    #[test]
    fn weighted_mean_uniform_weights_equal_unweighted() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        // All-ones weights reproduce the unweighted sample statistics.
        let w = [1.0; 5];
        assert_eq!(weighted_mean(&v, &w).unwrap(), 3.0);
        assert!(approx_eq(
            weighted_variance(&v, &w).unwrap(),
            2.5, // equals the unweighted sample variance
            1e-12
        ));
    }

    #[test]
    fn weighted_variance_frequency_weights() {
        // With frequency weights of 2 (each point counted twice), the
        // denominator becomes Σw − 1 = 9 and the numerator doubles.
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        let w = [2.0; 5];
        // Σ 2·(x−3)² = 2·10 = 20, over 10 − 1 = 9.
        assert!(approx_eq(
            weighted_variance(&v, &w).unwrap(),
            20.0 / 9.0,
            1e-12
        ));
    }

    #[test]
    fn weighted_validation() {
        assert!(matches!(
            weighted_mean(&[1.0, 2.0], &[1.0]),
            Err(StatError::LengthMismatch { .. })
        ));
        assert!(matches!(
            weighted_mean(&[1.0], &[-1.0]),
            Err(StatError::InvalidWeights)
        ));
        assert!(matches!(
            weighted_mean(&[1.0, 2.0], &[0.0, 0.0]),
            Err(StatError::InvalidWeights)
        ));
    }

    #[test]
    fn weighted_correlation_perfect_linear() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        let ys = [2.0, 4.0, 6.0, 8.0];
        let w = [1.0, 2.0, 1.0, 3.0];
        assert!(approx_eq(
            weighted_correlation(&xs, &ys, &w).unwrap(),
            1.0,
            1e-12
        ));
    }

    #[test]
    fn weighted_quantile_extremes_and_median() {
        let v = [1.0, 2.0, 3.0, 4.0];
        let w = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(weighted_quantile(&v, &w, 0.0).unwrap(), 1.0);
        assert_eq!(weighted_quantile(&v, &w, 1.0).unwrap(), 4.0);
        // Uniform weights, total 4, median target = 2 → boundary between 2 and 3.
        assert_eq!(weighted_quantile(&v, &w, 0.5).unwrap(), 2.0);
    }
}
