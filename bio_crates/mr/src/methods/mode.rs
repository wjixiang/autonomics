//! Mode-based estimators — `R/mr_mode.R`.
//!
//! The point estimate ([`mode_beta`]) is the grid location of maximum Gaussian
//! KDE density ([`crate::kde`]); it is deterministic and bit-aligned to R. The
//! standard error comes from a parametric bootstrap (`R/mr_mode.R:57 boot`) and
//! is reported as the MAD of the bootstrap distribution; it is therefore
//! RNG-dependent and not bit-identical to R.

use crate::dist::{pchisq_sf, pt_two_sided};
use crate::kde::density;
use crate::result::count_valid4;
use crate::{MrEstimate, Parameters, rnorm_one};
use rand::Rng;

const MAD_CONSTANT: f64 = 1.4826;

/// R's `mad(x)` (default `constant = 1.4826`): `median(|x − median(x)|) · c`.
fn mad(x: &[f64]) -> f64 {
    let m = median(x);
    let dev: Vec<f64> = x.iter().map(|v| (v - m).abs()).collect();
    median(&dev) * MAD_CONSTANT
}

/// R's `median(x)` (averages the two middle order statistics for even n).
fn median(x: &[f64]) -> f64 {
    let mut s = x.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

/// R's `sd(x)` — sample standard deviation, `n − 1` denominator.
fn sd_sample(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    if n < 2.0 {
        return f64::NAN;
    }
    let mean = x.iter().sum::<f64>() / n;
    let ss = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    (ss / (n - 1.0)).sqrt()
}

/// The R `beta(BetaIV.in, seBetaIV.in, phi)` function (`R/mr_mode.R:31`) for a
/// scalar `phi`: bandwidth `s = 0.9·min(sd, mad)/n^(1/5)`, `h = max(1e-8, s·phi)`,
/// point estimate = argmax grid location of the weighted Gaussian KDE.
/// The R `beta(BetaIV.in, seBetaIV.in, phi)` function (`R/mr_mode.R:31`) for a
/// scalar `phi`: bandwidth `s = 0.9·min(sd, mad)/n^(1/5)`, `h = max(1e-8, s·phi)`,
/// point estimate = argmax grid location of the weighted Gaussian KDE. Weights
/// are the **inverse variances** `seBetaIV.in⁻²` (R computes these inside
/// `beta()` before calling `density`).
fn mode_beta(betav: &[f64], seb: &[f64], phi: f64) -> f64 {
    let n = betav.len() as f64;
    let s = 0.9 * sd_sample(betav).min(mad(betav)) / n.powf(0.2);
    let h = (s * phi).max(1e-8);
    let w: Vec<f64> = seb.iter().map(|s| 1.0 / (s * s)).collect();
    density(betav, &w, h).argmax_x()
}

/// Variance of a ratio estimate (delta method, no Cov) — `R/mr.R:765`.
fn vbj(b_exp: f64, b_out: f64, se_exp: f64, se_out: f64) -> f64 {
    let be2 = b_exp * b_exp;
    se_out * se_out / be2 + b_out * b_out * se_exp * se_exp / (be2 * be2)
}

/// All five mode-method point estimates + bootstrap SEs for one
/// exposure/outcome pair (`R/mr_mode.R`, `mode_method = "all"`).
#[allow(dead_code)]
struct ModeAll {
    simple: f64,
    weighted: f64,
    penalised: f64,
    weighted_nome: f64,
    se_simple: f64,
    se_weighted: f64,
    se_penalised: f64,
    se_simple_nome: f64,
    se_weighted_nome: f64,
}

/// Compute the full mode result. `phi` is taken from `parameters.phi`; the
/// bootstrap uses `parameters.nboot` and `parameters.penk`.
fn mode_all<R: Rng>(
    b_exp: &[f64],
    b_out: &[f64],
    se_exp: &[f64],
    se_out: &[f64],
    parameters: &Parameters,
    rng: &mut R,
) -> ModeAll {
    let n = b_exp.len();
    let phi = parameters.phi;
    let penk = parameters.penk;
    let nboot = parameters.nboot;

    let beta_iv: Vec<f64> = (0..n).map(|i| b_out[i] / b_exp[i]).collect();
    let se_col1: Vec<f64> = (0..n)
        .map(|i| vbj(b_exp[i], b_out[i], se_exp[i], se_out[i]).sqrt())
        .collect();
    let se_col2: Vec<f64> = (0..n).map(|i| se_out[i] / b_exp[i].abs()).collect();
    let ones = vec![1.0; n];
    let weights: Vec<f64> = se_col1.iter().map(|s| 1.0 / (s * s)).collect();

    // ---- Point estimates (deterministic) ----
    let simple = mode_beta(&beta_iv, &ones, phi);
    let weighted = mode_beta(&beta_iv, &se_col1, phi);
    // Penalised: penalty against the weighted-mode estimate.
    let mut pen_weights = vec![0.0; n];
    for i in 0..n {
        let penalty = pchisq_sf(weights[i] * (beta_iv[i] - weighted).powi(2), 1.0);
        pen_weights[i] = weights[i] * (1.0_f64).min(penalty * penk);
    }
    let penalised = mode_beta(
        &beta_iv,
        &pen_weights
            .iter()
            .map(|p| (1.0 / p).sqrt())
            .collect::<Vec<_>>(),
        phi,
    );
    let weighted_nome = mode_beta(&beta_iv, &se_col2, phi);

    // ---- Bootstrap SEs (MAD across nboot replications) ----
    let mut b_simple = vec![0.0; nboot];
    let mut b_weighted = vec![0.0; nboot];
    let mut b_penalised = vec![0.0; nboot];
    let mut b_simple_nome = vec![0.0; nboot];
    let mut b_weighted_nome = vec![0.0; nboot];

    for k in 0..nboot {
        let mut boot = vec![0.0; n];
        let mut boot_nome = vec![0.0; n];
        for j in 0..n {
            boot[j] = beta_iv[j] + se_col1[j] * rnorm_one(rng);
            boot_nome[j] = beta_iv[j] + se_col2[j] * rnorm_one(rng);
        }
        b_simple[k] = mode_beta(&boot, &ones, phi);
        b_weighted[k] = mode_beta(&boot, &se_col1, phi);
        // Penalised bootstrap uses this iteration's weighted-mode estimate.
        let wm_boot = b_weighted[k];
        let mut pw = vec![0.0; n];
        for j in 0..n {
            let penalty = pchisq_sf(weights[j] * (boot[j] - wm_boot).powi(2), 1.0);
            pw[j] = weights[j] * (1.0_f64).min(penalty * penk);
        }
        let pw_se: Vec<f64> = pw.iter().map(|p| (1.0 / p).sqrt()).collect();
        b_penalised[k] = mode_beta(&boot, &pw_se, phi);
        b_simple_nome[k] = mode_beta(&boot_nome, &ones, phi);
        b_weighted_nome[k] = mode_beta(&boot_nome, &se_col2, phi);
    }

    ModeAll {
        simple,
        weighted,
        penalised,
        weighted_nome,
        se_simple: mad(&b_simple),
        se_weighted: mad(&b_weighted),
        se_penalised: mad(&b_penalised),
        se_simple_nome: mad(&b_simple_nome),
        se_weighted_nome: mad(&b_weighted_nome),
    }
}

/// Build an [`MrEstimate`] for one mode variant.
fn mode_estimate(b: f64, se: f64, n: usize) -> MrEstimate {
    let df = (n.saturating_sub(1)) as f64;
    let pval = pt_two_sided(b / se, df);
    MrEstimate::from_core(b, se, pval, n)
}

/// `mr_simple_mode` (`R/mr_mode.R:298`).
pub fn mr_simple_mode(
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
    let mut rng = crate::default_rng();
    let all = mode_all(b_exp, b_out, se_exp, se_out, parameters, &mut rng);
    mode_estimate(all.simple, all.se_simple, n)
}

/// `mr_weighted_mode` (`R/mr_mode.R:259`).
pub fn mr_weighted_mode(
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
    let mut rng = crate::default_rng();
    let all = mode_all(b_exp, b_out, se_exp, se_out, parameters, &mut rng);
    mode_estimate(all.weighted, all.se_weighted, n)
}

/// `mr_simple_mode_nome` (`R/mr_mode.R:380`).
pub fn mr_simple_mode_nome(
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
    let mut rng = crate::default_rng();
    let all = mode_all(b_exp, b_out, se_exp, se_out, parameters, &mut rng);
    // Simple mode (NOME) shares the simple-mode point estimate; SE from NOME
    // bootstrap column.
    mode_estimate(all.simple, all.se_simple_nome, n)
}

/// `mr_weighted_mode_nome` (`R/mr_mode.R:339`).
pub fn mr_weighted_mode_nome(
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
    let mut rng = crate::default_rng();
    let all = mode_all(b_exp, b_out, se_exp, se_out, parameters, &mut rng);
    mode_estimate(all.weighted_nome, all.se_weighted_nome, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_mad() {
        assert!((median(&[1.0, 2.0, 3.0, 4.0, 5.0]) - 3.0).abs() < 1e-12);
        assert!((median(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < 1e-12);
        // mad of 1..5: deviations |1..5| from median 3 = [2,1,0,1,2], median 1
        assert!((mad(&[1.0, 2.0, 3.0, 4.0, 5.0]) - 1.4826).abs() < 1e-10);
    }

    #[test]
    fn mode_needs_three_snps() {
        let e = mr_simple_mode(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[0.1, 0.1],
            &[0.1, 0.1],
            &Parameters::default(),
        );
        assert!(e.b.is_nan());
    }
}
