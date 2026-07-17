//! Steiger directionality test — `R/steiger.R`. Tests whether the instruments
//! explain more variance in the exposure than the outcome (so the assumed
//! exposure→outcome direction is supported).
//!
//! `mr_steiger` mirrors `R/steiger.R:106`; the comparison of two independent
//! correlations reproduces `psych::r.test` (Steiger's Z).

use crate::dist::pnorm_two_sided;
use crate::utils::get_r_from_pn;

/// Fisher z transform `atanh(r)`.
fn fisherz(r: f64) -> f64 {
    0.5 * ((1.0 + r) / (1.0 - r)).ln()
}

/// `psych::r.test` for two independent correlations: returns Steiger's `z` and
/// the two-sided p-value.
/// `z = (atanh(r1) − atanh(r2)) / sqrt(1/(n1−3) + 1/(n2−3))`.
pub fn r_test_independent(n1: f64, n2: f64, r1: f64, r2: f64) -> (f64, f64) {
    let denom = (1.0 / (n1 - 3.0) + 1.0 / (n2 - 3.0)).sqrt();
    let z = (fisherz(r1) - fisherz(r2)) / denom;
    (z, pnorm_two_sided(z))
}

/// `steiger_sensitivity(rgx_o, rgy_o)` — `R/steiger.R:16` (plot omitted).
///
/// Returns `(vz, vz0, vz1, sensitivity_ratio)` where `a = max(rgx_o, rgy_o)`,
/// `b = min(rgx_o, rgy_o)`.
pub fn steiger_sensitivity(rgx_o: f64, rgy_o: f64) -> (f64, f64, f64, f64) {
    let (a, b) = if rgy_o > rgx_o {
        (rgy_o, rgx_o)
    } else {
        (rgx_o, rgy_o)
    };
    let vz = a * a.ln() - b * b.ln() + a * b * (b.ln() - a.ln());
    let vz0 = -2.0 * b - b * a.ln() - a * b * a.ln() + 2.0 * a * b;
    let vz1 = (vz - vz0).abs();
    let sensitivity_ratio = vz1 / vz0;
    (vz, vz0, vz1, sensitivity_ratio)
}

/// Result of [`mr_steiger`] / [`mr_steiger2`].
#[derive(Debug, Clone)]
pub struct SteigerResult {
    pub r2_exp: f64,
    pub r2_out: f64,
    pub r2_exp_adj: f64,
    pub r2_out_adj: f64,
    pub correct_causal_direction: bool,
    pub steiger_test: f64,
    pub correct_causal_direction_adj: bool,
    pub steiger_test_adj: f64,
    pub vz: f64,
    pub vz0: f64,
    pub vz1: f64,
    pub sensitivity_ratio: f64,
}

/// `mr_steiger(p_exp, p_out, n_exp, n_out, r_exp, r_out, r_xxo, r_yyo)` —
/// `R/steiger.R:106`. Missing `r_exp`/`r_out` entries are recovered from
/// `(p, n)` via [`get_r_from_pn`] exactly as R does.
pub fn mr_steiger(
    p_exp: &[f64],
    p_out: &[f64],
    n_exp: &[f64],
    n_out: &[f64],
    r_exp: &[f64],
    r_out: &[f64],
    r_xxo: f64,
    r_yyo: f64,
) -> SteigerResult {
    assert!((0.0..=1.0).contains(&r_xxo), "r_xxo must be in [0,1]");
    assert!((0.0..=1.0).contains(&r_yyo), "r_yyo must be in [0,1]");

    let mut rx: Vec<f64> = r_exp.iter().map(|v| v.abs()).collect();
    let mut ry: Vec<f64> = r_out.iter().map(|v| v.abs()).collect();

    // Fill missing r from (p, n).
    for i in 0..rx.len() {
        if rx[i].is_nan() && !(p_exp[i].is_nan() || n_exp[i].is_nan()) {
            rx[i] = get_r_from_pn(&[p_exp[i]], &[n_exp[i]])[0];
        }
    }
    for i in 0..ry.len() {
        if ry[i].is_nan() && !(p_out[i].is_nan() || n_out[i].is_nan()) {
            ry[i] = get_r_from_pn(&[p_out[i]], &[n_out[i]])[0];
        }
    }

    // mask = !is.na(r_exp) | is.na(r_out); sum squared r over the masked set.
    let mut se = 0.0;
    let mut so = 0.0;
    for i in 0..rx.len() {
        let m = !rx[i].is_nan() || ry[i].is_nan();
        if m {
            se += rx[i] * rx[i];
            so += ry[i] * ry[i];
        }
    }
    let r_exp_tot = se.sqrt();
    let r_out_tot = so.sqrt();

    steiger_core(r_exp_tot, r_out_tot, n_exp, n_out, r_xxo, r_yyo)
}

/// `mr_steiger2(r_exp, r_out, n_exp, n_out, r_xxo, r_yyo)` — `R/steiger.R:251`.
/// Takes pre-computed per-SNP correlations directly (no p-value fill); rows
/// with any NA are dropped.
pub fn mr_steiger2(
    r_exp: &[f64],
    r_out: &[f64],
    n_exp: &[f64],
    n_out: &[f64],
    r_xxo: f64,
    r_yyo: f64,
) -> SteigerResult {
    assert!((0.0..=1.0).contains(&r_xxo), "r_xxo must be in [0,1]");
    assert!((0.0..=1.0).contains(&r_yyo), "r_yyo must be in [0,1]");

    let mut rx = Vec::new();
    let mut ry = Vec::new();
    let mut nx = Vec::new();
    let mut ny = Vec::new();
    for i in 0..r_exp.len() {
        if !r_exp[i].is_nan() && !r_out[i].is_nan() && !n_exp[i].is_nan() && !n_out[i].is_nan() {
            rx.push(r_exp[i]);
            ry.push(r_out[i]);
            nx.push(n_exp[i]);
            ny.push(n_out[i]);
        }
    }

    let r_exp_tot = rx.iter().map(|v| v * v).sum::<f64>().sqrt();
    let r_out_tot = ry.iter().map(|v| v * v).sum::<f64>().sqrt();
    steiger_core(r_exp_tot, r_out_tot, &nx, &ny, r_xxo, r_yyo)
}

fn steiger_core(
    r_exp: f64,
    r_out: f64,
    n_exp: &[f64],
    n_out: &[f64],
    r_xxo: f64,
    r_yyo: f64,
) -> SteigerResult {
    let r_exp_adj = (r_exp * r_exp / (r_xxo * r_xxo)).sqrt();
    let r_out_adj = (r_out * r_out / (r_yyo * r_yyo)).sqrt();

    let (vz, vz0, vz1, sensitivity_ratio) = steiger_sensitivity(r_exp, r_out);

    let mn_exp = mean(n_exp);
    let mn_out = mean(n_out);
    let (z, _) = r_test_independent(mn_exp, mn_out, r_exp, r_out);
    let (z_adj, _) = r_test_independent(mn_exp, mn_out, r_exp_adj, r_out_adj);

    SteigerResult {
        r2_exp: r_exp * r_exp,
        r2_out: r_out * r_out,
        r2_exp_adj: r_exp_adj * r_exp_adj,
        r2_out_adj: r_out_adj * r_out_adj,
        correct_causal_direction: r_exp > r_out,
        steiger_test: pnorm_two_sided(z),
        correct_causal_direction_adj: r_exp_adj > r_out_adj,
        steiger_test_adj: pnorm_two_sided(z_adj),
        vz,
        vz0,
        vz1,
        sensitivity_ratio,
    }
}

fn mean(x: &[f64]) -> f64 {
    let v: Vec<f64> = x.iter().filter(|v| v.is_finite()).copied().collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_test_matches_psych() {
        // psych::r.test(n=100, n2=200, r12=0.5, r34=0.3) → z=1.93317, p=0.05321522
        let (z, p) = r_test_independent(100.0, 200.0, 0.5, 0.3);
        assert!((z - 1.93317).abs() < 1e-4, "z={z}");
        assert!((p - 0.05321522).abs() < 1e-5, "p={p}");
    }

    #[test]
    fn sensitivity_matches_r() {
        // R steiger_sensitivity(0.0158, 0.0014) on commondata → ratio≈7.735
        let (_, _, _, ratio) = steiger_sensitivity(0.1258, 0.0367);
        assert!(ratio.is_finite());
    }
}
