//! Wald ratio estimator — `R/mr.R:337 mr_wald_ratio`.

use crate::{MrEstimate, Parameters, dist::pnorm_two_sided};

/// `mr_wald_ratio(b_exp, b_out, se_exp, se_out, parameters)`.
///
/// Single-instrument estimator: `b = b_out / b_exp`, `se = se_out / |b_exp|`,
/// `pval = 2·pnorm(|b|/se)`. Requires exactly **one** SNP — more than one
/// returns an all-NA estimate (R warns and returns NA).
pub fn mr_wald_ratio(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    _parameters: &Parameters,
) -> MrEstimate {
    if b_exp.len() != 1 {
        // R: warning + list(b=NA, se=NA, pval=NA, nsnp=NA) when length != 1.
        let mut e = MrEstimate::na();
        e.nsnp = 0;
        return e;
    }
    let _ = (b_out.len(), se_exp.len(), se_out.len());
    let b = b_out[0] / b_exp[0];
    let se = se_out[0] / b_exp[0].abs();
    let pval = pnorm_two_sided(b / se);
    MrEstimate::from_core(b, se, pval, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wald_basic() {
        let e = mr_wald_ratio(&[2.0], &[1.0], &[0.1], &[0.2], &Parameters::default());
        // b = 1/2 = 0.5; se = 0.2/2 = 0.1
        assert!((e.b - 0.5).abs() < 1e-12);
        assert!((e.se - 0.1).abs() < 1e-12);
        assert!((e.pval - pnorm_two_sided(5.0)).abs() < 1e-12);
        assert_eq!(e.nsnp, 1);
    }

    #[test]
    fn wald_multi_snp_is_na() {
        let e = mr_wald_ratio(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[0.1, 0.1],
            &[0.1, 0.1],
            &Parameters::default(),
        );
        assert!(e.b.is_nan());
    }
}
