//! Weighted-median estimators — `R/mr.R:759 mr_weighted_median`,
//! `:791 mr_simple_median`, `:900 mr_penalised_weighted_median`, plus the
//! shared helpers `weighted_median` (`:820`) and `weighted_median_bootstrap`
//! (`:850`).
//!
//! The point estimate [`weighted_median`] is deterministic; the standard error
//! comes from a parametric bootstrap and is therefore RNG-dependent (R and
//! Rust cannot share an RNG stream). Callers may inject an `&mut Rng`.

use crate::dist::{pchisq_sf, pnorm_two_sided};
use crate::result::count_valid4;
use crate::{MrEstimate, Parameters, default_rng, rnorm_one};
use rand::Rng;

/// `weighted_median(b_iv, weights)` — the deterministic weighted-median point
/// estimate (`R/mr.R:820`). Linearly interpolates between the two order
/// statistics whose cumulative weight straddles 0.5.
pub fn weighted_median(b_iv: &[f64], weights: &[f64]) -> f64 {
    let n = b_iv.len();
    if n == 0 {
        return f64::NAN;
    }
    // Stable ascending sort by b_iv (R order() breaks ties by position).
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| {
        b_iv[a]
            .partial_cmp(&b_iv[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total: f64 = weights.iter().sum();
    if total <= 0.0 || total.is_nan() {
        return f64::NAN;
    }
    // cumsum(weights.order) - 0.5*weights.order, normalised by total.
    let mut wsum = vec![0.0; n];
    let mut acc = 0.0;
    for (k, &j) in idx.iter().enumerate() {
        acc += weights[j];
        wsum[k] = (acc - 0.5 * weights[j]) / total;
    }
    // below = max index with wsum < 0.5.
    let mut below: isize = -1;
    for k in 0..n {
        if wsum[k] < 0.5 {
            below = k as isize;
        }
    }
    if below < 0 {
        // First cumulative weight already >= 0.5 (degenerate): return smallest.
        return b_iv[idx[0]];
    }
    let lo = below as usize;
    let hi = lo + 1;
    if hi >= n {
        return b_iv[idx[lo]];
    }
    let bv_lo = b_iv[idx[lo]];
    let bv_hi = b_iv[idx[hi]];
    bv_lo + (bv_hi - bv_lo) * (0.5 - wsum[lo]) / (wsum[hi] - wsum[lo])
}

/// Variance of a ratio estimate `b_out / b_exp` by the delta method (no Cov):
/// `VBj = se_out²/b_exp² + b_out²·se_exp²/b_exp⁴` (`R/mr.R:765`).
fn vbj(b_exp: f64, b_out: f64, se_exp: f64, se_out: f64) -> f64 {
    let be2 = b_exp * b_exp;
    se_out * se_out / be2 + b_out * b_out * se_exp * se_exp / (be2 * be2)
}

/// `weighted_median_bootstrap(b_exp, b_out, se_exp, se_out, weights, nboot, rng)`
/// (`R/mr.R:850`). Returns the sample SD (`n-1` denominator) of `nboot`
/// bootstrap weighted medians.
pub fn weighted_median_bootstrap<R: Rng>(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    weights: &[f64],
    nboot: usize,
    rng: &mut R,
) -> f64 {
    let nsnp = b_exp.len();
    let mut med = vec![f64::NAN; nboot];
    for b in 0..nboot {
        let mut biv = vec![f64::NAN; nsnp];
        for j in 0..nsnp {
            let xe = b_exp[j] + se_exp[j] * rnorm_one(rng);
            let yo = b_out[j] + se_out[j] * rnorm_one(rng);
            biv[j] = yo / xe;
        }
        med[b] = weighted_median(&biv, weights);
    }
    // R's sd() — sample standard deviation with n-1 denominator, na.rm=TRUE.
    let valid: Vec<f64> = med.iter().filter(|v| v.is_finite()).copied().collect();
    let m = valid.len() as f64;
    if m < 2.0 {
        return f64::NAN;
    }
    let mean = valid.iter().sum::<f64>() / m;
    let ss = valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    (ss / (m - 1.0)).sqrt()
}

/// `mr_weighted_median` (`R/mr.R:759`). Uses the default seeded RNG for the
/// bootstrap SE.
pub fn mr_weighted_median(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    parameters: &Parameters,
) -> MrEstimate {
    let n = b_exp.len();
    if count_valid4(b_exp, b_out, se_exp, se_out).unwrap_or(0) < 3 {
        let mut e = MrEstimate::na();
        e.nsnp = n;
        return e;
    }
    let b_iv: Vec<f64> = (0..n).map(|i| b_out[i] / b_exp[i]).collect();
    let inv_vbj: Vec<f64> = (0..n)
        .map(|i| 1.0 / vbj(b_exp[i], b_out[i], se_exp[i], se_out[i]))
        .collect();
    let b = weighted_median(&b_iv, &inv_vbj);
    let mut rng = default_rng();
    let se = weighted_median_bootstrap(
        b_exp,
        b_out,
        se_exp,
        se_out,
        &inv_vbj,
        parameters.nboot,
        &mut rng,
    );
    let pval = pnorm_two_sided(b / se);
    MrEstimate {
        b,
        se,
        pval,
        nsnp: n,
        ..Default::default()
    }
}

/// `mr_simple_median` (`R/mr.R:791`) — equal weights `1/n`.
pub fn mr_simple_median(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    parameters: &Parameters,
) -> MrEstimate {
    let n = b_exp.len();
    if count_valid4(b_exp, b_out, se_exp, se_out).unwrap_or(0) < 3 {
        let mut e = MrEstimate::na();
        e.nsnp = n;
        return e;
    }
    let b_iv: Vec<f64> = (0..n).map(|i| b_out[i] / b_exp[i]).collect();
    let w = vec![1.0 / n as f64; n];
    let b = weighted_median(&b_iv, &w);
    let mut rng = default_rng();
    let se =
        weighted_median_bootstrap(b_exp, b_out, se_exp, se_out, &w, parameters.nboot, &mut rng);
    let pval = pnorm_two_sided(b / se);
    MrEstimate::from_core(b, se, pval, n)
}

/// `mr_penalised_weighted_median` (`R/mr.R:900`).
pub fn mr_penalised_weighted_median(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    parameters: &Parameters,
) -> MrEstimate {
    let n = b_exp.len();
    if count_valid4(b_exp, b_out, se_exp, se_out).unwrap_or(0) < 3 {
        let mut e = MrEstimate::na();
        e.nsnp = n;
        return e;
    }
    let beta_iv: Vec<f64> = (0..n).map(|i| b_out[i] / b_exp[i]).collect();
    let inv_vbj: Vec<f64> = (0..n)
        .map(|i| 1.0 / vbj(b_exp[i], b_out[i], se_exp[i], se_out[i]))
        .collect();
    // IVW estimate (for the penalty reference is the weighted-median estimate).
    let bwm = mr_weighted_median(b_exp, b_out, se_exp, se_out, parameters).b;

    let mut pen_weights = vec![0.0; n];
    for i in 0..n {
        let penalty = pchisq_sf(inv_vbj[i] * (beta_iv[i] - bwm).powi(2), 1.0);
        pen_weights[i] = inv_vbj[i] * (1.0_f64).min(penalty * parameters.penk);
    }
    let b = weighted_median(&beta_iv, &pen_weights);
    let mut rng = default_rng();
    let se = weighted_median_bootstrap(
        b_exp,
        b_out,
        se_exp,
        se_out,
        &pen_weights,
        parameters.nboot,
        &mut rng,
    );
    let pval = pnorm_two_sided(b / se);
    MrEstimate::from_core(b, se, pval, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-12)
    }

    #[test]
    fn weighted_median_odd_equal_weights_is_median() {
        // Equal weights on 5 points → plain median (middle order statistic).
        let m = weighted_median(&[1.0, 3.0, 2.0, 5.0, 4.0], &[1.0; 5]);
        // For n=5 equal weights, cumsum normalised crosses 0.5 between order
        // stats 3 and 4 (1-indexed) → interpolates around 3.
        assert!(approx(m, 3.0, 1e-9));
    }

    #[test]
    fn weighted_median_matches_known_value() {
        // R: weighted_median(c(1,2,3,4,5,6), c(0.1,0.1,0.2,0.2,0.2,0.2)) == 4
        let m = weighted_median(
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            &[0.1, 0.1, 0.2, 0.2, 0.2, 0.2],
        );
        // wsum = [0.05,0.15,0.30,0.50,0.70,0.90]; below=2 (0.30<0.5); interpolate
        // between order stats at idx 2 (3) and 3 (4): 3 + 1*(0.5-0.3)/(0.5-0.3)=4
        assert!(approx(m, 4.0, 1e-9));
    }

    #[test]
    fn penalised_median_runs() {
        let p = Parameters::default();
        let e = mr_penalised_weighted_median(
            &[0.1, 0.2, 0.3, 0.4, 0.5],
            &[0.05, 0.1, 0.15, 0.21, 0.27],
            &[0.01; 5],
            &[0.02; 5],
            &p,
        );
        assert!(e.b.is_finite());
        assert!(e.se > 0.0);
    }
}
