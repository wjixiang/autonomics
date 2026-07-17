//! The uniform result type returned by every MR estimator.
//!
//! R's per-method functions return named lists with a varying subset of
//! `{b, se, pval, nsnp, Q, Q_df, Q_pval, b_i, se_i, pval_i, ...}`. We collapse
//! these into a single struct: the always-present core (`b`, `se`, `pval`,
//! `nsnp`) plus `Option<f64>` fields for the heterogeneity (`Q*`) and Egger
//! intercept (`*_i`) quantities. Methods that don't compute a field leave it
//! `None` (R leaves it absent / `NA`).

use crate::na;

/// A single MR method's estimate.
#[derive(Debug, Clone, Default)]
pub struct MrEstimate {
    /// Causal-effect point estimate.
    pub b: f64,
    /// Standard error of `b`.
    pub se: f64,
    /// Two-sided p-value for `b`.
    pub pval: f64,
    /// Number of SNPs used.
    pub nsnp: usize,

    /// Cochran's Q heterogeneity statistic (when computed).
    pub q: Option<f64>,
    /// Degrees of freedom for Q.
    pub q_df: Option<f64>,
    /// p-value for Q.
    pub q_pval: Option<f64>,

    /// MR-Egger intercept estimate (when computed).
    pub b_i: Option<f64>,
    /// SE of the intercept.
    pub se_i: Option<f64>,
    /// p-value of the intercept.
    pub pval_i: Option<f64>,
}

impl MrEstimate {
    /// An "all-NA" estimate, matching the null lists R returns when there are
    /// too few valid SNPs (e.g. `list(b=NA, se=NA, pval=NA, nsnp=NA)`).
    pub fn na() -> Self {
        Self {
            b: f64::NAN,
            se: f64::NAN,
            pval: f64::NAN,
            nsnp: 0,
            q: None,
            q_df: None,
            q_pval: None,
            b_i: None,
            se_i: None,
            pval_i: None,
        }
    }

    /// Build an estimate from a raw `(b, se, pval, nsnp)` tuple.
    pub fn from_core(b: f64, se: f64, pval: f64, nsnp: usize) -> Self {
        Self {
            b,
            se,
            pval,
            nsnp,
            ..Default::default()
        }
    }
}

/// Count SNPs valid across all four input vectors, mirroring R's
/// `sum(!is.na(b_exp) & !is.na(b_out) & !is.na(se_exp) & !is.na(se_out))`.
pub(crate) fn count_valid4(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
) -> crate::Result<usize> {
    let n = b_exp.len();
    if b_out.len() != n || se_exp.len() != n || se_out.len() != n {
        return Err(crate::MrError::LengthMismatch(format!(
            "count_valid4: lengths {}, {}, {}, {}",
            b_exp.len(),
            b_out.len(),
            se_exp.len(),
            se_out.len()
        )));
    }
    let mut c = 0usize;
    for i in 0..n {
        if na::is_valid(&b_exp[i])
            && na::is_valid(&b_out[i])
            && na::is_valid(&se_exp[i])
            && na::is_valid(&se_out[i])
        {
            c += 1;
        }
    }
    Ok(c)
}
