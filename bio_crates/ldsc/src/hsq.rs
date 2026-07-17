//! LD Score Regression for SNP-heritability (h²). A faithful Rust/faer port of
//! LDSC's `Hsq` (`ldscore/regressions.py:336-535`) and its base class
//! `LD_Score_Regression` (`regressions.py:142-309`).
//!
//! Given a joined `DataFrame` of GWAS Z-scores, per-SNP sample sizes, per-SNP
//! per-annotation LD Scores, and per-SNP weight LD Scores — plus the
//! per-annotation L2-summed `M` — [`estimate_h2`] returns total h² with its
//! block-jackknife standard error, the regression intercept, the LD Score
//! regression ratio, λ_GC, and the mean χ².
//!
//! ## Model
//! `E[χ²ⱼ] = 1 + (Nⱼ / M) · h² · Lⱼ`. We regress χ² on a design whose k-th
//! column is `Nⱼ·ref_ldⱼₖ / N̄` (plus an intercept), with IRWLS weights and a
//! block jackknife for the standard errors. h² is recovered as
//! `h² = Σₖ Mₖ·βₖ` where `βₖ = estₖ / N̄`.

use datafusion::prelude::DataFrame;

use crate::{LdscError, Result};

use crate::ingest::{self, HsqArrays};

/// Column names in the joined input `DataFrame`.
pub struct HsqColumns<'a> {
    /// SNP identifier column (used by the caller to join; not read here).
    pub snp: &'a str,
    /// Z-score column (χ² = Z²).
    pub z: &'a str,
    /// Per-SNP sample-size column.
    pub n: &'a str,
    /// One or more per-SNP LD-Score columns (one per annotation). Order must
    /// match the order of `m` passed to [`estimate_h2`].
    pub ref_ld: Vec<&'a str>,
    /// Per-SNP weight LD-Score column (the `--w-ld` file's LD column).
    pub w_ld: &'a str,
}

/// h² regression output.
#[derive(Debug, Clone)]
pub struct HsqResult {
    /// Total SNP-heritability `Σₖ Mₖ·βₖ`.
    pub h2: f64,
    /// Standard error of `h2` (block jackknife).
    pub h2_se: f64,
    /// Regression intercept (the χ² inflation not explained by LD). `None` if a
    /// constrained intercept was used.
    pub intercept: Option<f64>,
    /// Standard error of the intercept.
    pub intercept_se: Option<f64>,
    /// LD Score regression ratio `(intercept − 1) / (meanχ² − 1)` — the share of
    /// χ² inflation attributable to confounding rather than polygenicity. `None`
    /// when the intercept is constrained or `meanχ² ≤ 1`.
    pub ratio: Option<f64>,
    /// Standard error of the ratio.
    pub ratio_se: Option<f64>,
    /// Mean per-SNP χ².
    pub mean_chisq: f64,
    /// Genomic-control λ: `median(χ²) / 0.4549`.
    pub lambda_gc: f64,
    /// Number of SNPs in the regression.
    pub n_snp: usize,
    /// Per-annotation coefficients `βₖ` (length `n_annot`).
    pub coef: Vec<f64>,
    /// Standard errors of `coef`.
    pub coef_se: Vec<f64>,
}

/// Estimate h² from a joined GWAS + LD-Score `DataFrame`.
///
/// * `df` — already joined on SNP (sumstats ⨝ ref_ld ⨝ w_ld).
/// * `cols` — names of the Z, N, ref_ld, w_ld columns.
/// * `m` — per-annotation L2-summed `M` values (from the `.l2.M_5_50` file),
///   length `n_annot`. **Not** the SNP count.
/// * `n_blocks` — block-jackknife block count (LDSC default 200).
/// * `intercept` — `None` for a free intercept (the normal case), or a fixed
///   value for a constrained-intercept regression.
pub async fn estimate_h2(
    df: DataFrame,
    cols: HsqColumns<'_>,
    m: &[f64],
    n_blocks: usize,
    intercept: Option<f64>,
) -> Result<HsqResult> {
    let HsqArrays { z, n, ref_ld, w_ld } = ingest::to_arrays(df, &cols).await?;
    let n_annot = ref_ld.ncols();
    if m.len() != n_annot {
        return Err(LdscError::DimensionMismatch(format!(
            "estimate_h2: m has length {} but there are {n_annot} ref_ld columns",
            m.len()
        )));
    }

    // χ² = Z².
    let chisq: Vec<f64> = z.iter().map(|zj| zj * zj).collect();

    // Apply the same defaults Python's `sumstats.estimate_h2` uses before calling
    // `reg.Hsq`: single annotation + free intercept → two-step estimator
    // (cutoff 30); multiple annotations → `old_weights`. This makes the
    // DataFrame entry produce the same numbers as the canonical Python path.
    let old_weights = n_annot > 1;
    let two_step = if n_annot == 1 && intercept.is_none() {
        Some(30.0)
    } else {
        None
    };

    let hsq = crate::regress::Hsq::new(
        &chisq,
        &ref_ld,
        &w_ld,
        &n,
        m,
        n_blocks,
        intercept,
        two_step,
        old_weights,
    )?;

    Ok(HsqResult {
        h2: hsq.reg.tot,
        h2_se: hsq.reg.tot_se,
        intercept: hsq.reg.intercept,
        intercept_se: hsq.reg.intercept_se,
        ratio: hsq.ratio,
        ratio_se: hsq.ratio_se,
        mean_chisq: hsq.mean_chisq,
        lambda_gc: hsq.lambda_gc,
        n_snp: chisq.len(),
        coef: hsq.reg.coef,
        coef_se: hsq.reg.coef_se,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Median, matching numpy's `np.median` (averages the two middle values for
    /// even length). Kept as a local test helper to pin the λ_GC convention.
    fn median(v: &mut [f64]) -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        }
    }

    #[test]
    fn median_matches_numpy() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [1.0, 2.0, 3.0, 4.0]), 2.5);
    }

    #[test]
    fn columns_struct_builds() {
        let _c = HsqColumns {
            snp: "snp",
            z: "z",
            n: "n",
            ref_ld: vec!["ld"],
            w_ld: "w",
        };
    }

    #[test]
    fn error_type_compiles() {
        let _: Result<()> = Err(LdscError::InvalidInput("test".into()));
    }
}
