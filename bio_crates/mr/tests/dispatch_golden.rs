//! End-to-end `mr()` dispatch on the harmonised commondata fixture. Expected
//! `b` values are the R `test_mr.R` golden values (egger 0.5025, ivw 0.4459,
//! weighted median 0.3870, simple mode 0.3402, weighted mode 0.3791).

use mr::harmonise::HarmoniseOutput;

fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for ch in line.chars() {
        match ch {
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

fn load_rows() -> Vec<HarmoniseOutput> {
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/commondata_kept.csv"
    ))
    .unwrap();
    let mut out = Vec::new();
    for line in csv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f = split(line);
        // SNP, id.exp, id.out, beta.exp, beta.out, se.exp, se.out, mr_keep, ...
        out.push(HarmoniseOutput {
            snp: f[0].clone(),
            id_exposure: f[1].clone(),
            id_outcome: f[2].clone(),
            beta_exposure: f[3].parse().unwrap(),
            beta_outcome: f[4].parse().unwrap(),
            se_exposure: f[5].parse().unwrap(),
            se_outcome: f[6].parse().unwrap(),
            effect_allele_exposure: "A".into(),
            other_allele_exposure: None,
            effect_allele_outcome: "A".into(),
            other_allele_outcome: None,
            eaf_exposure: None,
            eaf_outcome: None,
            remove: false,
            palindromic: false,
            ambiguous: false,
            mr_keep: f[7] == "TRUE",
        });
    }
    out
}

fn b_for(rows: &[mr::MrResultRow], method: &str) -> f64 {
    rows.iter().find(|r| r.method == method).unwrap().b
}

#[test]
fn mr_default_matches_r() {
    let dat = load_rows();
    // Point estimates don't depend on nboot; use a small value for speed.
    let mut p = mr::default_parameters();
    p.nboot = 10;
    let res = mr::mr(&dat, &p, &[]).unwrap();
    assert_eq!(res.len(), 5, "expected 5 default-method rows");

    // R test_mr.R golden b values (tolerance 1e-3 as in the R test).
    assert!((b_for(&res, "MR Egger") - 0.5025).abs() < 1e-3);
    assert!((b_for(&res, "Weighted median") - 0.3870).abs() < 1e-3);
    assert!((b_for(&res, "Inverse variance weighted") - 0.4459).abs() < 1e-3);
    assert!((b_for(&res, "Simple mode") - 0.3402).abs() < 1e-2); // R uses 1e-1
    assert!((b_for(&res, "Weighted mode") - 0.3791).abs() < 1e-2); // R uses 1e-1
}

#[test]
fn mr_wald_only_for_single_snp_or_sole_method() {
    let dat = load_rows();
    // 79 SNPs + default (multi-method) list → wald_ratio excluded.
    let mut p = mr::default_parameters();
    p.nboot = 10;
    let res = mr::mr(&dat, &p, &[]).unwrap();
    assert!(res.iter().all(|r| r.method != "Wald ratio"));

    // Explicit wald-only on the first SNP.
    let one: Vec<HarmoniseOutput> = dat.into_iter().take(1).collect();
    let res2 = mr::mr(&one, &mr::default_parameters(), &["mr_wald_ratio"]).unwrap();
    assert_eq!(res2.len(), 1);
    assert_eq!(res2[0].method, "Wald ratio");
}
