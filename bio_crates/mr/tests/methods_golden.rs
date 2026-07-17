//! Golden validation of the deterministic MR estimators against R outputs.
//!
//! Input: `tests/data/commondata_kept.csv` (the 79 mr_keep SNPs from
//! `TwoSampleMR/inst/extdata/test_commondata.RData`). Expected values were
//! produced by sourcing `R/mr.R` + `R/mr_mode.R` in R and printing each
//! method's result (see the plan's fixture-export step).

use mr::{Parameters, methods};

/// Minimal quoted-CSV field splitter.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

/// Load the harmonised fixture, returning (b_exp, b_out, se_exp, se_out).
fn load() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/commondata_kept.csv"
    );
    let txt = std::fs::read_to_string(path).expect("fixture csv");
    let mut bx = Vec::new();
    let mut bo = Vec::new();
    let mut sx = Vec::new();
    let mut so = Vec::new();
    for line in txt.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f = split_csv(line);
        // columns: SNP, id.exposure, id.outcome, beta.exposure, beta.outcome,
        //          se.exposure, se.outcome, mr_keep, ...
        bx.push(f[3].parse::<f64>().unwrap());
        bo.push(f[4].parse::<f64>().unwrap());
        sx.push(f[5].parse::<f64>().unwrap());
        so.push(f[6].parse::<f64>().unwrap());
    }
    (bx, bo, sx, so)
}

fn approx(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-12)
}

#[test]
fn ivw_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_ivw(&bx, &bo, &sx, &so, &Parameters::default());
    assert!(approx(e.b, 0.445909096953, 1e-7), "b={}", e.b);
    assert!(approx(e.se, 0.058983018763, 1e-7), "se={}", e.se);
    assert_eq!(e.nsnp, 79);
    assert!(approx(e.q.unwrap(), 143.650840925034, 1e-6));
    assert!(approx(e.q_df.unwrap(), 78.0, 1e-9));
    assert!(approx(e.q_pval.unwrap(), 8.72842e-6, 1e-4));
}

#[test]
fn ivw_fe_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_ivw_fe(&bx, &bo, &sx, &so, &Parameters::default());
    assert!(approx(e.b, 0.445909096953, 1e-7));
    assert!(approx(e.se, 0.043463051161, 1e-7), "se={}", e.se);
}

#[test]
fn ivw_mre_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_ivw_mre(&bx, &bo, &sx, &so, &Parameters::default());
    assert!(approx(e.b, 0.445909096953, 1e-7));
    assert!(approx(e.se, 0.058983018763, 1e-7));
}

#[test]
fn uwr_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_uwr(&bx, &bo, &sx, &so, &Parameters::default());
    assert!(approx(e.b, 0.341368729452, 1e-7), "b={}", e.b);
    assert!(approx(e.se, 3.772618035971, 1e-6), "se={}", e.se);
}

#[test]
fn egger_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_egger_regression(&bx, &bo, &sx, &so, &Parameters::default());
    assert!(approx(e.b, 0.502493509730, 1e-7), "b={}", e.b);
    assert!(approx(e.se, 0.143960561723, 1e-7), "se={}", e.se);
    assert!(approx(e.pval, 0.000801258992, 1e-7), "pval={}", e.pval);
    assert!(approx(e.b_i.unwrap(), -0.001719304085, 1e-8));
    assert!(approx(e.se_i.unwrap(), 0.003985962365, 1e-8));
    assert!(approx(e.pval_i.unwrap(), 0.667426592113, 1e-7));
    assert!(approx(e.q.unwrap(), 143.304576132034, 1e-6));
    assert!(approx(e.q_df.unwrap(), 77.0, 1e-9));
}

#[test]
fn median_point_estimates_match_r() {
    // Point estimates are deterministic; SEs are bootstrap (RNG) — check finite.
    let (bx, bo, sx, so) = load();
    let p = Parameters::default();

    let sm = methods::mr_simple_median(&bx, &bo, &sx, &so, &p);
    assert!(
        approx(sm.b, 0.392723577236, 1e-6),
        "simple median b={}",
        sm.b
    );
    assert!(sm.se.is_finite() && sm.se > 0.0);

    let wm = methods::mr_weighted_median(&bx, &bo, &sx, &so, &p);
    assert!(
        approx(wm.b, 0.387006481544, 1e-6),
        "weighted median b={}",
        wm.b
    );
    assert!(wm.se.is_finite() && wm.se > 0.0);

    let pm = methods::mr_penalised_weighted_median(&bx, &bo, &sx, &so, &p);
    assert!(
        approx(pm.b, 0.383263018541, 1e-6),
        "penalised median b={}",
        pm.b
    );
    assert!(pm.se.is_finite() && pm.se > 0.0);
}

#[test]
fn wald_first_snp_matches_r() {
    let (bx, bo, sx, so) = load();
    let e = methods::mr_wald_ratio(
        &bx[..1],
        &bo[..1],
        &sx[..1],
        &so[..1],
        &Parameters::default(),
    );
    assert!(approx(e.b, -0.790108695652, 1e-7), "b={}", e.b);
    assert!(approx(e.se, 0.534755434783, 1e-7));
}

#[test]
fn mode_point_estimates_match_r() {
    // Point estimates are deterministic; SEs are bootstrap (RNG). Use small
    // nboot to keep the bootstrap cheap — only `b` is asserted.
    let (bx, bo, sx, so) = load();
    let p = Parameters {
        nboot: 25,
        ..Parameters::default()
    };

    let sm = methods::mr_simple_mode(&bx, &bo, &sx, &so, &p);
    assert!(
        (sm.b - 0.340155428665).abs() < 2e-3,
        "simple mode b={}",
        sm.b
    );

    let wm = methods::mr_weighted_mode(&bx, &bo, &sx, &so, &p);
    assert!(
        (wm.b - 0.379090990504).abs() < 2e-3,
        "weighted mode b={}",
        wm.b
    );
}
