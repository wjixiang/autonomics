//! Pure summary-statistic utilities ported from `R/add_rsq.r`, `R/query.R` and
//! `R/rucker.R`. These convert between p-values / R² / sample sizes, recover
//! SEs, handle log-odds (binary-trait) correlations, and compute I².
//!
//! Each function documents its R source line for traceability.

use crate::dist::{pf_sf, qnorm_lower};

// ---------------------------------------------------------------------------
// get_p_from_r2n  — R/add_rsq.r:157
// ---------------------------------------------------------------------------

/// `get_p_from_r2n(r2, n)` — p-value from R² and sample size.
/// `fval = r2*(n-2)/(1-r2)`; `pval = pf(fval, 1, n-1, lower.tail=FALSE)`.
pub fn get_p_from_r2n(r2: f64, n: f64) -> f64 {
    let fval = r2 * (n - 2.0) / (1.0 - r2);
    pf_sf(fval, 1.0, n - 1.0)
}

// ---------------------------------------------------------------------------
// get_r_from_pn  — R/add_rsq.r:175
// ---------------------------------------------------------------------------

/// `get_r_from_pn(p, n)` — approximate per-SNP |r| from p-values and sample
/// sizes.
///
/// R broadcasts a scalar `n` across `p`; we do the same when `n.len() == 1`.
/// Returns `sqrt(R²)`. A p-value of exactly 0 yields `NaN` (R: "P-value of 0
/// cannot be converted to R value").
///
/// We invert `get_p_from_r2n` by bisecting on `R² ∈ [0,1)` using the **survival
/// function `pf_sf`** rather than `qf`. R's `qf(p, …, lower.tail=FALSE)` computes
/// the upper-tail quantile directly and stays accurate for tiny p; a naive
/// `inverse_cdf(1 − p)` (as a generic F quantile gives) collapses to
/// `inverse_cdf(1.0) = +∞` once `p < 2.2e-16`, because `1 − p` rounds to 1 in
/// `f64`. `pf_sf` has no such cancellation, so the bisection is accurate across
/// the whole p-value range (genome-wide-significant down to `1e-300`).
pub fn get_r_from_pn(p: &[f64], n: &[f64]) -> Vec<f64> {
    let len = p.len();
    let n_rep: Vec<f64> = if n.len() == 1 {
        vec![n[0]; len]
    } else {
        n.to_vec()
    };
    debug_assert_eq!(n_rep.len(), len);

    let mut out = vec![f64::NAN; len];
    for i in 0..len {
        let pi = p[i];
        let ni = n_rep[i];
        if pi == 0.0 {
            // R: R2 <- NA with a warning. Leave NaN.
            continue;
        }
        if !pi.is_finite() || !(0.0..=1.0).contains(&pi) || ni <= 2.0 {
            continue;
        }
        out[i] = solve_r2_for_p(pi, ni).max(0.0).sqrt();
    }
    out
}

/// Solve `get_p_from_r2n(x, n) = target_p` for `x ∈ [0, 1)` by bisection on the
/// survival function `pf_sf`. `g(x) = pf_sf(r2·(n-2)/(1-r2), 1, n-1)` decreases
/// monotonically from 1 (x=0) to 0 (x→1).
///
/// Convergence is on the bracket `[lo, hi]` (i.e. on R² itself), not on `g`:
/// for tiny `target_p`, `g` underflows to 0 well before the true root, so an
/// absolute `g`-tolerance would stop far too early.
fn solve_r2_for_p(target_p: f64, n: f64) -> f64 {
    let g = |x: f64| {
        let fval = x * (n - 2.0) / (1.0 - x);
        pf_sf(fval, 1.0, n - 1.0)
    };
    let mut lo = 0.0f64;
    let mut hi = 1.0f64 - 1e-13;
    // Shrink hi until g(hi) is finite.
    let mut guard = 0;
    while !g(hi).is_finite() && guard < 300 {
        hi = 0.5 * (lo + hi);
        guard += 1;
    }
    // g decreases in x: g(lo) ≈ 1 > target, g(hi) ≈ 0 ≤ target.
    for _ in 0..200 {
        if (hi - lo).abs() <= 1e-15 {
            break;
        }
        let mid = 0.5 * (lo + hi);
        let gm = g(mid);
        if !gm.is_finite() || gm <= target_p {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    0.5 * (lo + hi)
}

// ---------------------------------------------------------------------------
// get_r_from_bsen  — R/add_rsq.r:217
// ---------------------------------------------------------------------------

/// `get_r_from_bsen(b, se, n)` — signed per-SNP r from effect, SE, sample size.
/// `Fval = (b/se)²`; `r = sqrt(Fval/(n-2+Fval)) * sign(b)`.
pub fn get_r_from_bsen(b: &[f64], se: &[f64], n: &[f64]) -> Vec<f64> {
    b.iter()
        .zip(se.iter())
        .zip(n.iter())
        .map(|((&bi, &sei), &ni)| {
            let fval = (bi / sei).powi(2);
            let r2 = fval / (ni - 2.0 + fval);
            r2.max(0.0).sqrt() * bi.signum()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// get_se  — R/query.R:247
// ---------------------------------------------------------------------------

/// `get_se(eff, pval)` — recover SE from effect size and p-value.
/// `|eff| / |qnorm(pval/2)|`.
pub fn get_se(eff: &[f64], pval: &[f64]) -> Vec<f64> {
    eff.iter()
        .zip(pval.iter())
        .map(|(&e, &p)| e.abs() / qnorm_lower(p / 2.0).abs())
        .collect()
}

// ---------------------------------------------------------------------------
// effective_n  — R/add_rsq.r:389
// ---------------------------------------------------------------------------

/// `effective_n(ncase, ncontrol)` — case/control effective sample size.
/// `2 / (1/ncase + 1/ncontrol)`.
pub fn effective_n(ncase: f64, ncontrol: f64) -> f64 {
    2.0 / (1.0 / ncase + 1.0 / ncontrol)
}

// ---------------------------------------------------------------------------
// Isq  — R/rucker.R:11
// ---------------------------------------------------------------------------

/// `Isq(y, s)` — I² heterogeneity statistic.
/// `k = length(y)`; `w = 1/s²`; `mu = Σ w y / Σ w`; `Q = Σ w (y-mu)²`;
/// `Isq = max(0, (Q-(k-1))/Q)`.
pub fn isq(y: &[f64], s: &[f64]) -> f64 {
    let k = y.len() as f64;
    if k < 2.0 {
        return f64::NAN;
    }
    let w: Vec<f64> = s.iter().map(|si| 1.0 / (si * si)).collect();
    let sumw: f64 = w.iter().sum();
    let mu: f64 = y.iter().zip(&w).map(|(yi, wi)| wi * yi).sum::<f64>() / sumw;
    let q: f64 = y
        .iter()
        .zip(&w)
        .map(|(yi, wi)| wi * (yi - mu).powi(2))
        .sum();
    let val = (q - (k - 1.0)) / q;
    if val < 0.0 { 0.0 } else { val }
}

// ---------------------------------------------------------------------------
// allele_frequency  — R/add_rsq.r:331
// ---------------------------------------------------------------------------

/// `allele_frequency(g)` — allele frequency from 0/1/2 dosages.
/// `(Σ(g==1) + 2·Σ(g==2)) / (2·Σ(!is.na(g)))`. NaN entries are ignored.
pub fn allele_frequency(g: &[f64]) -> f64 {
    let valid = g.iter().filter(|v| !v.is_nan()).count() as f64;
    if valid == 0.0 {
        return f64::NAN;
    }
    let mut num = 0.0;
    for &gi in g {
        if gi.is_nan() {
            continue;
        }
        if gi == 1.0 {
            num += 1.0;
        } else if gi == 2.0 {
            num += 2.0;
        }
    }
    num / (2.0 * valid)
}

// ---------------------------------------------------------------------------
// get_population_allele_frequency  — R/add_rsq.r:345
// ---------------------------------------------------------------------------

/// `get_population_allele_frequency(af, prop, odds_ratio, prevalence)`.
///
/// Solves the 2×2 contingency for the case-allele cell `z`, then returns
/// `af_controls·(1-prevalence) + af_cases·prevalence`. All four arguments are
/// parallel slices of equal length.
pub fn get_population_allele_frequency(
    af: &[f64],
    prop: &[f64],
    odds_ratio: &[f64],
    prevalence: &[f64],
) -> Vec<f64> {
    let len = odds_ratio.len();
    let eps = 1e-15;
    let mut z = vec![0.0; len];
    for i in 0..len {
        let a = odds_ratio[i] - 1.0;
        let b = (af[i] + prop[i]) * (1.0 - odds_ratio[i]) - 1.0;
        let c = odds_ratio[i] * af[i] * prop[i];
        if a.abs() < eps {
            // linear
            z[i] = -c / b;
        } else {
            // quadratic, choose the root that yields a valid 2×2 table.
            let d = (b * b - 4.0 * a * c).max(0.0);
            let sqrt_d = d.sqrt();
            let two_a = 2.0 * a;
            let z_pos = (-b + sqrt_d) / two_a;
            let z_neg = (-b - sqrt_d) / two_a;
            let tol = -1e-7;
            let valid_pos = z_pos >= tol
                && (prop[i] - z_pos) >= tol
                && (af[i] - z_pos) >= tol
                && (1.0 + z_pos - af[i] - prop[i]) >= tol;
            z[i] = if valid_pos { z_pos } else { z_neg };
        }
    }
    let af_controls: Vec<f64> = (0..len).map(|i| (af[i] - z[i]) / (1.0 - prop[i])).collect();
    let af_cases: Vec<f64> = (0..len).map(|i| z[i] / prop[i]).collect();
    (0..len)
        .map(|i| af_controls[i] * (1.0 - prevalence[i]) + af_cases[i] * prevalence[i])
        .collect()
}

/// `model` argument for [`get_r_from_lor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LorModel {
    /// Logit model (variance `π²/3`). Matches R `model = "logit"`.
    Logit,
    /// Probit model (variance 1). Matches R `model = "probit"`.
    Probit,
}

// ---------------------------------------------------------------------------
// get_r_from_lor  — R/add_rsq.r:245
// ---------------------------------------------------------------------------

/// `get_r_from_lor(lor, af, ncase, ncontrol, prevalence, model, correction)`.
///
/// Estimates the (signed) proportion of liability variance explained by a SNP
/// from a log-odds ratio (Lee et al. 2012, eq. 10). Scalar `ncase`, `ncontrol`
/// and `prevalence` are broadcast across `lor` exactly as R does.
pub fn get_r_from_lor(
    lor: &[f64],
    af: &[f64],
    ncase: &[f64],
    ncontrol: &[f64],
    prevalence: &[f64],
    model: LorModel,
    correction: bool,
) -> Vec<f64> {
    let len = lor.len();
    let ncase_rep = broadcast(ncase, len);
    let ncontrol_rep = broadcast(ncontrol, len);
    let prev_rep = broadcast(prevalence, len);

    let ve = match model {
        LorModel::Logit => std::f64::consts::PI * std::f64::consts::PI / 3.0,
        LorModel::Probit => 1.0,
    };
    let prop: Vec<f64> = (0..len)
        .map(|i| ncase_rep[i] / (ncase_rep[i] + ncontrol_rep[i]))
        .collect();
    let or: Vec<f64> = lor.iter().map(|l| l.exp()).collect();
    let popaf = get_population_allele_frequency(af, &prop, &or, &prev_rep);

    let mut out = vec![f64::NAN; len];
    for i in 0..len {
        let vg = lor[i] * lor[i] * popaf[i] * (1.0 - popaf[i]);
        let r = vg / (vg + ve);
        let r = if correction { r / 0.58 } else { r };
        out[i] = r.max(0.0).sqrt() * lor[i].signum();
    }
    out
}

/// Broadcast a length-1 slice to `len`, or require `slice.len() == len`.
fn broadcast(slice: &[f64], len: usize) -> Vec<f64> {
    if slice.len() == 1 {
        vec![slice[0]; len]
    } else {
        assert_eq!(
            slice.len(),
            len,
            "broadcast: slice length {} != target {len}",
            slice.len()
        );
        slice.to_vec()
    }
}

// ---------------------------------------------------------------------------
// contingency  — R/add_rsq.r:300
// ---------------------------------------------------------------------------

/// `contingency(af, prop, odds_ratio, eps)` — 2×2 contingency table(s) from
/// marginal parameters and an odds ratio. Returns all tables whose entries are
/// all ≥ 0 (R's `y[,,i]` selection). Each table is row-major `[[a, b], [c, d]]`.
pub fn contingency(af: f64, prop: f64, odds_ratio: f64, eps: f64) -> Vec<[[f64; 2]; 2]> {
    let a = odds_ratio - 1.0;
    let b = (af + prop) * (1.0 - odds_ratio) - 1.0;
    let c = odds_ratio * af * prop;

    let zs: Vec<f64> = if a.abs() < eps {
        vec![-c / b]
    } else {
        let d = b * b - 4.0 * a * c;
        if d < eps * eps {
            vec![]
        } else {
            let s = [1.0, -1.0];
            s.iter()
                .map(|&sgn| (-b + sgn * d.max(0.0).sqrt()) / (2.0 * a))
                .collect()
        }
    };

    let mut tables = Vec::new();
    for &z in &zs {
        let row1 = [z, prop - z];
        let row2 = [af - z, 1.0 + z - af - prop];
        // R: zapsmall then `all(u >= 0)`. We test >= 0 after rounding tiny noise.
        let all_nonneg = [row1[0], row1[1], row2[0], row2[1]]
            .iter()
            .all(|v| *v >= -1e-12);
        if all_nonneg {
            tables.push([row1, row2]);
        }
    }
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-12)
    }

    #[test]
    fn effective_n_value() {
        // effective_n(100, 100) = 100; (50,200) = 2/(1/50+1/200)=80
        assert!(approx(effective_n(100.0, 100.0), 100.0, 1e-12));
        assert!(approx(effective_n(50.0, 200.0), 80.0, 1e-12));
    }

    #[test]
    fn isq_value() {
        // Identical effects → Q = 0 → Isq clamped to 0.
        assert!(isq(&[1.0, 1.0, 1.0], &[0.1, 0.1, 0.1]).abs() < 1e-12);
        // Heterogeneous effects → Isq > 0.
        assert!(isq(&[1.0, 2.0, 3.0, 4.0], &[0.1, 0.1, 0.1, 0.1]) > 0.0);
    }

    #[test]
    fn get_p_from_r2n_and_pn_roundtrip() {
        // pick r2=0.01, n=1000 → p → recover r ≈ sqrt(0.01)
        let n = 1000.0;
        let r2 = 0.01;
        let p = get_p_from_r2n(r2, n);
        let r = get_r_from_pn(&[p], &[n])[0];
        assert!(approx(r * r, r2, 1e-6));
    }

    #[test]
    fn get_r_from_pn_extreme_p() {
        // R: get_r_from_pn(8.07049e-23, 338829) ≈ 0.0173 (tiny R²).
        let r = get_r_from_pn(&[8.07049e-23], &[338829.0])[0];
        assert!(r < 0.05, "extreme-p r should be tiny, got {r}");
        assert!(r > 0.0);
    }

    #[test]
    fn get_r_from_bsen_matches_formula() {
        // b=0.1, se=0.01, n=1000 → F=(10)^2=100 → r2=100/(998+100)
        let r = get_r_from_bsen(&[0.1], &[0.01], &[1000.0])[0];
        let fval = 100.0_f64;
        let expect = (fval / (1000.0 - 2.0 + fval)).sqrt();
        assert!(approx(r, expect, 1e-12));
    }

    #[test]
    fn allele_frequency_basic() {
        // dosages [0,1,2,2] → (1 + 2*2)/(2*4) = 5/8
        assert!(approx(
            allele_frequency(&[0.0, 1.0, 2.0, 2.0]),
            5.0 / 8.0,
            1e-12
        ));
        // NaNs ignored: [0,NaN,2] → (0 + 2*1)/(2*2) = 0.5
        assert!(approx(allele_frequency(&[0.0, f64::NAN, 2.0]), 0.5, 1e-12));
    }
}
