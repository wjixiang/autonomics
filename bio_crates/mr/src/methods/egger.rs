//! MR-Egger regression — `R/mr.R:567 mr_egger_regression`.
//!
//! Orients all instruments to positive exposure effect (`sign0(b_exp)`,
//! treating 0 as +1), then fits `lm(b_out ~ b_exp, weights = 1/se_out^2)`.
//! Slope = causal estimate, intercept = pleiotropy. SEs are divided by
//! `min(1, sigma)`; p-values use a t distribution with `n−2` df; Rücker's Q is
//! `sigma^2 · (n−2)`.

use crate::dist::{pchisq_sf, pt_two_sided};
use crate::linalg::wlm;
use crate::result::count_valid4;
use crate::{MrEstimate, Parameters};

/// R's `sign0`: `sign(x)` but 0 → 1.
fn sign0(x: f64) -> f64 {
    if x == 0.0 { 1.0 } else { x.signum() }
}

/// `mr_egger_regression(b_exp, b_out, se_exp, se_out, parameters)`.
///
/// Returns an all-NA estimate when fewer than 3 valid SNPs are present, or when
/// the design is collinear (R: "Collinearities in MR Egger").
pub fn mr_egger_regression(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    let n = b_exp.len();
    if count_valid4(b_exp, b_out, se_exp, se_out).unwrap_or(0) < 3 {
        let mut e = MrEstimate::na();
        e.nsnp = n;
        return e;
    }

    // Orient to positive exposure effect.
    let bx: Vec<f64> = b_exp.iter().map(|&b| b.abs()).collect();
    let by: Vec<f64> = b_out
        .iter()
        .zip(b_exp.iter())
        .map(|(&o, &e)| o * sign0(e))
        .collect();
    let w: Vec<f64> = se_out.iter().map(|s| 1.0 / (s * s)).collect();

    let fit = match wlm(&bx, &by, &w, true) {
        Ok(f) => f,
        Err(_) => {
            // Collinear design — R returns the null list.
            let mut e = MrEstimate::na();
            e.nsnp = n;
            return e;
        }
    };

    let df = (n - 2) as f64;
    let sigma = fit.sigma;
    let b = fit.coef[1]; // slope
    let se = fit.se[1] / sigma.min(1.0);
    let pval = pt_two_sided(b / se, df);
    let b_i = fit.coef[0]; // intercept
    let se_i = fit.se[0] / sigma.min(1.0);
    let pval_i = pt_two_sided(b_i / se_i, df);

    let q = sigma * sigma * df;
    let q_pval = pchisq_sf(q, df);

    MrEstimate {
        b,
        se,
        pval,
        nsnp: n,
        q: Some(q),
        q_df: Some(df),
        q_pval: Some(q_pval),
        b_i: Some(b_i),
        se_i: Some(se_i),
        pval_i: Some(pval_i),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egger_needs_three_snps() {
        let e = mr_egger_regression(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[0.1, 0.1],
            &[0.1, 0.1],
            &Parameters::default(),
        );
        assert!(e.b.is_nan());
    }

    #[test]
    fn egger_zero_exposure_treated_as_positive() {
        // sign0(0) = 1, so a zero b_exp is kept as 0 (abs), b_out unchanged.
        let s = sign0(0.0);
        assert_eq!(s, 1.0);
        assert_eq!(sign0(-3.0), -1.0);
        assert_eq!(sign0(3.0), 1.0);
    }
}
