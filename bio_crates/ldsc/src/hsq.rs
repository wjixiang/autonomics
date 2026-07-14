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
use faer::Mat;

use crate::{LdscError, Result};

use crate::ingest::{self, HsqArrays};
use crate::irwls::{self, IrwlsOutput};
use crate::jackknife::{self, JackknifeResult};

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

/// Median of chi², matching numpy's `np.median` (averages the two middle values
/// for even length).
fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
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
    let nrows = z.len();
    // Count of functional annotation columns
    let n_annot = ref_ld.ncols();
    if m.len() != n_annot {
        return Err(LdscError::DimensionMismatch(format!(
            "estimate_h2: m has length {} but there are {n_annot} ref_ld columns",
            m.len()
        )));
    }
    let free_intercept = intercept.is_none();
    let fixed_intercept = intercept.unwrap_or(1.0);

    // χ² and summary stats.
    let y: Vec<f64> = z.iter().map(|zj| zj * zj).collect();
    let mean_chisq: f64 = y.iter().sum::<f64>() / (nrows as f64);
    let lambda_gc = median(&mut y.clone()) / 0.4549;

    let nbar: f64 = n.iter().sum::<f64>() / (nrows as f64);
    // Total LD per SNP (sum over annotations, raw — used by the weight formula).
    let ld_tot: Vec<f64> = (0..nrows)
        .map(|i| (0..n_annot).map(|k| ref_ld[(i, k)]).sum())
        .collect();
    let m_tot: f64 = m.iter().sum();

    // Initial aggregate h² (regressions.py:237-244).
    let mean_ldn: f64 = (0..nrows).map(|i| ld_tot[i] * n[i]).sum::<f64>() / (nrows as f64);
    let initial_hsq = if mean_ldn > 0.0 {
        m_tot * (mean_chisq - fixed_intercept) / mean_ldn
    } else {
        0.0
    };

    // Build the design matrix: N·ref_ld / Nbar for each annotation, plus an
    // intercept column when free.
    let p = if free_intercept { n_annot + 1 } else { n_annot };
    let x = Mat::from_fn(nrows, p, |i, j| {
        if j < n_annot {
            n[i] * ref_ld[(i, j)] / nbar
        } else {
            1.0 // intercept column
        }
    });
    let yp: Vec<f64> = if free_intercept {
        y.clone()
    } else {
        y.iter().map(|yi| yi - fixed_intercept).collect()
    };

    // IRWLS → weighted design/response for the jackknife.
    let IrwlsOutput { x: xw, y: yw } = irwls::irwls(
        &x,
        &yp,
        &ld_tot,
        &w_ld,
        &n,
        m_tot,
        nbar,
        n_annot,
        free_intercept,
        initial_hsq,
        fixed_intercept,
    )?;

    // Block jackknife.
    let JackknifeResult { est, cov, .. } = jackknife::jackknife_fast(&xw, &yw, n_blocks)?;

    // Coefficients and covariance (per-annotation slopes, divided by Nbar).
    let mut coef = vec![0.0; n_annot];
    let mut coef_cov = vec![0.0; n_annot * n_annot];
    for a in 0..n_annot {
        coef[a] = est[a] / nbar;
        for b in 0..n_annot {
            coef_cov[a * n_annot + b] = cov[(a, b)] / (nbar * nbar);
        }
    }
    let coef_se: Vec<f64> = (0..n_annot)
        .map(|a| (coef_cov[a * n_annot + a]).max(0.0).sqrt())
        .collect();

    // Per-annotation and total h²: cat_k = M_k·β_k, tot = Σ cat_k,
    // tot_cov = Σ_{a,b} M_a M_b coef_cov[a][b]  (regressions.py:271-283).
    let mut h2 = 0.0;
    let mut tot_cov = 0.0;
    for a in 0..n_annot {
        h2 += m[a] * coef[a];
        for b in 0..n_annot {
            tot_cov += m[a] * m[b] * coef_cov[a * n_annot + b];
        }
    }
    let h2_se = tot_cov.max(0.0).sqrt();

    // Intercept.
    let (intercept_est, intercept_se, ratio, ratio_se) = if free_intercept {
        let int_est = est[n_annot];
        let int_se = cov[(n_annot, n_annot)].max(0.0).sqrt();
        if mean_chisq > 1.0 {
            let denom = mean_chisq - 1.0;
            (
                Some(int_est),
                Some(int_se),
                Some((int_est - 1.0) / denom),
                Some(int_se / denom),
            )
        } else {
            (Some(int_est), Some(int_se), None, None)
        }
    } else {
        (intercept, None, None, None)
    };

    Ok(HsqResult {
        h2,
        h2_se,
        intercept: intercept_est,
        intercept_se,
        ratio,
        ratio_se,
        mean_chisq,
        lambda_gc,
        n_snp: nrows,
        coef,
        coef_se,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
