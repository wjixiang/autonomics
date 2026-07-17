//! Inverse-variance-weighted family — `R/mr.R:986` (`mr_ivw`), `:1031`
//! (`mr_uwr`), `:1075` (`mr_ivw_mre`), `:1117` (`mr_ivw_fe`).
//!
//! All four fit a through-origin weighted regression of `b_out` on `b_exp`
//! (`lm(b_out ~ -1 + b_exp, weights = 1/se_out^2)`; `mr_uwr` omits the weights).
//! They differ only in the standard-error correction:
//!
//! | variant   | SE                              |
//! |-----------|---------------------------------|
//! | `mr_ivw`  | `se_raw / min(1, sigma)`        |
//! | `mr_ivw_fe` | `se_raw / sigma`             |
//! | `mr_ivw_mre` | `se_raw` (no correction)    |
//! | `mr_uwr`  | `se_raw / min(1, sigma)` (unweighted) |

use crate::dist::{pchisq_sf, pnorm_two_sided};
use crate::linalg::wlm;
use crate::result::count_valid4;
use crate::{MrEstimate, Parameters};

/// How the IVW standard error is corrected for (under-)dispersion.
#[derive(Clone, Copy)]
enum SeCorrection {
    /// `mr_ivw` / `mr_uwr`: divide by `min(1, sigma)`.
    MinOneSigma,
    /// `mr_ivw_fe`: divide by `sigma`.
    Sigma,
    /// `mr_ivw_mre`: no correction.
    None,
}

fn ivw_core(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    weighted: bool,
    corr: SeCorrection,
) -> MrEstimate {
    let n = b_exp.len();
    if count_valid4(b_exp, b_out, se_exp, se_out).unwrap_or(0) < 2 {
        let mut e = MrEstimate::na();
        e.nsnp = n;
        return e;
    }
    let w: Vec<f64> = if weighted {
        se_out.iter().map(|s| 1.0 / (s * s)).collect()
    } else {
        vec![1.0; n]
    };

    let fit = match wlm(b_exp, b_out, &w, false) {
        Ok(f) => f,
        Err(_) => {
            let mut e = MrEstimate::na();
            e.nsnp = n;
            return e;
        }
    };

    let b = fit.slope();
    let se_raw = fit.se[0];
    let sigma = fit.sigma;
    let se = match corr {
        SeCorrection::MinOneSigma => se_raw / sigma.min(1.0),
        SeCorrection::Sigma => se_raw / sigma,
        SeCorrection::None => se_raw,
    };
    let pval = pnorm_two_sided(b / se);

    let q_df = (n - 1) as f64;
    let q = sigma * sigma * q_df;
    let q_pval = pchisq_sf(q, q_df);

    MrEstimate {
        b,
        se,
        pval,
        nsnp: n,
        q: Some(q),
        q_df: Some(q_df),
        q_pval: Some(q_pval),
        ..Default::default()
    }
}

/// `mr_ivw` — default multiplicative-random-effects IVW (`R/mr.R:986`).
pub fn mr_ivw(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    ivw_core(
        b_exp,
        b_out,
        se_exp,
        se_out,
        true,
        SeCorrection::MinOneSigma,
    )
}

/// `mr_ivw_fe` — fixed-effects IVW (`R/mr.R:1117`).
pub fn mr_ivw_fe(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    ivw_core(b_exp, b_out, se_exp, se_out, true, SeCorrection::Sigma)
}

/// `mr_ivw_mre` — multiplicative random effects, no under-dispersion correction
/// (`R/mr.R:1075`).
pub fn mr_ivw_mre(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    ivw_core(b_exp, b_out, se_exp, se_out, true, SeCorrection::None)
}

/// `mr_uwr` — unweighted regression (`R/mr.R:1031`).
pub fn mr_uwr(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    ivw_core(
        b_exp,
        b_out,
        se_exp,
        se_out,
        false,
        SeCorrection::MinOneSigma,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-12)
    }

    #[test]
    fn ivw_two_snp_closed_form() {
        // Through-origin WLS with 2 SNPs, weights 1/se_out^2.
        // x=[1,2], y=[1,2], se_out=[0.1,0.1] → perfect slope 1.
        let e = mr_ivw(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[0.1, 0.1],
            &[0.1, 0.1],
            &Parameters::default(),
        );
        assert!(approx(e.b, 1.0, 1e-9));
        assert!(approx(e.q_df.unwrap(), 1.0, 1e-12));
        // sigma=0 (perfect fit) → Q=0, Q_pval=1
        assert!(approx(e.q.unwrap(), 0.0, 1e-12));
    }

    #[test]
    fn too_few_snps_is_na() {
        let e = mr_ivw(&[1.0], &[1.0], &[0.1], &[0.1], &Parameters::default());
        assert!(e.b.is_nan());
    }
}
