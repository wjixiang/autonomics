//! Heterogeneity (Cochran's Q) and horizontal-pleiotropy testing —
//! `R/heterogeneity.R`.
//!
//! These are thin orchestration layers over [`crate::methods::mr_ivw`] and
//! [`crate::methods::mr_egger_regression`], which already compute Q statistics
//! and the Egger intercept. `mr_heterogeneity` runs the methods whose
//! `mr_method_list` entry has both `heterogeneity_test` and `use_by_default`
//! set — IVW and MR-Egger by default.

use crate::{Parameters, methods};

/// One row of `mr_heterogeneity` output.
#[derive(Debug, Clone)]
pub struct HeterogeneityRow {
    pub method: String,
    pub q: f64,
    pub q_df: f64,
    pub q_pval: f64,
}

/// `mr_heterogeneity(b_exp, b_out, se_exp, se_out, parameters)` — Q statistics
/// from IVW and MR-Egger (the default heterogeneity-test methods).
///
/// Returns one row per method whose Q is non-NA (matching R's
/// `subset(het_tab, !(is.na(Q) & is.na(Q_df) & is.na(Q_pval)))`).
pub fn mr_heterogeneity(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    parameters: &Parameters,
) -> Vec<HeterogeneityRow> {
    let mut out = Vec::new();
    let ivw = methods::mr_ivw(b_exp, b_out, se_exp, se_out, parameters);
    if let (Some(q), Some(df), Some(p)) = (ivw.q, ivw.q_df, ivw.q_pval) {
        if q.is_finite() || df.is_finite() || p.is_finite() {
            out.push(HeterogeneityRow {
                method: "Inverse variance weighted".into(),
                q,
                q_df: df,
                q_pval: p,
            });
        }
    }
    let egger = methods::mr_egger_regression(b_exp, b_out, se_exp, se_out, parameters);
    if let (Some(q), Some(df), Some(p)) = (egger.q, egger.q_df, egger.q_pval) {
        if q.is_finite() || df.is_finite() || p.is_finite() {
            out.push(HeterogeneityRow {
                method: "MR Egger".into(),
                q,
                q_df: df,
                q_pval: p,
            });
        }
    }
    out
}

/// One row of `mr_pleiotropy_test` output.
#[derive(Debug, Clone)]
pub struct PleiotropyRow {
    pub egger_intercept: f64,
    pub se: f64,
    pub pval: f64,
}

/// `mr_pleiotropy_test(b_exp, b_out, se_exp, se_out)` — the MR-Egger intercept
/// (horizontal-pleiotropy test). Returns `None` when Egger cannot be fit
/// (fewer than 3 valid SNPs).
pub fn mr_pleiotropy_test(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
) -> Option<PleiotropyRow> {
    let e = methods::mr_egger_regression(b_exp, b_out, se_exp, se_out, &Parameters::default());
    let (bi, sei, pi) = (e.b_i?, e.se_i?, e.pval_i?);
    if bi.is_nan() && sei.is_nan() && pi.is_nan() {
        return None;
    }
    Some(PleiotropyRow {
        egger_intercept: bi,
        se: sei,
        pval: pi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heterogeneity_returns_ivw_and_egger() {
        let b_exp = &[0.1, 0.2, 0.3, 0.4, 0.5];
        let b_out = &[0.05, 0.1, 0.16, 0.18, 0.27];
        let se = &[0.01; 5];
        let rows = mr_heterogeneity(b_exp, b_out, se, se, &Parameters::default());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].method, "Inverse variance weighted");
        assert_eq!(rows[1].method, "MR Egger");
    }

    #[test]
    fn pleiotropy_returns_intercept() {
        let b_exp = &[0.1, 0.2, 0.3, 0.4, 0.5];
        let b_out = &[0.05, 0.1, 0.16, 0.18, 0.27];
        let se = &[0.01; 5];
        let p = mr_pleiotropy_test(b_exp, b_out, se, se).unwrap();
        assert!(p.egger_intercept.is_finite());
    }
}
