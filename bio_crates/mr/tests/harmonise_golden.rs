//! End-to-end validation of `harmonise_data` against R's `harmonise_data`
//! output. Raw inputs (`raw_exposure.csv`, `raw_outcome.csv`) and the expected
//! harmonised data frame (`expected_harmonised.csv`) are the 79-SNP
//! `ieu-a-2` → `ieu-a-7` pair from `test_commondata.RData`.

use mr::harmonise::{HarmoniseInput, harmonise_data};
use std::collections::HashMap;

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

fn load_csv(path: &str) -> Vec<Vec<String>> {
    let txt = std::fs::read_to_string(path).unwrap();
    txt.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(split)
        .collect()
}

fn opt_allele(s: &str) -> Option<String> {
    if s.is_empty() || s == "NA" {
        None
    } else {
        Some(s.to_string())
    }
}

fn opt_f64(s: &str) -> Option<f64> {
    if s.is_empty() || s == "NA" {
        None
    } else {
        Some(s.parse().unwrap())
    }
}

#[test]
fn harmonise_matches_r() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");
    let exp = load_csv(&format!("{base}raw_exposure.csv"));
    let out = load_csv(&format!("{base}raw_outcome.csv"));

    // Index outcome rows by SNP.
    let mut out_by_snp: HashMap<String, &Vec<String>> = HashMap::new();
    for r in &out {
        out_by_snp.insert(r[0].clone(), r);
    }

    let mut inputs = Vec::new();
    for r in &exp {
        let o = match out_by_snp.get(&r[0]) {
            Some(o) => *o,
            None => continue,
        };
        inputs.push(HarmoniseInput {
            snp: r[0].clone(),
            id_exposure: r[1].clone(),
            beta_exposure: r[2].parse().unwrap(),
            se_exposure: r[3].parse().unwrap(),
            effect_allele_exposure: opt_allele(&r[4]),
            other_allele_exposure: opt_allele(&r[5]),
            eaf_exposure: opt_f64(&r[6]),
            beta_outcome: o[2].parse().unwrap(),
            se_outcome: o[3].parse().unwrap(),
            effect_allele_outcome: opt_allele(&o[4]),
            other_allele_outcome: opt_allele(&o[5]),
            eaf_outcome: opt_f64(&o[6]),
            id_outcome: o[1].clone(),
        });
    }

    let got = harmonise_data(&inputs);
    let expected = load_csv(&format!("{base}expected_harmonised.csv"));
    // exp columns: SNP, beta.exp, beta.out, eaf.exp, eaf.out, ea.exp, oa.exp, ea.out, oa.out, mr_keep, remove, palindromic, ambiguous
    let mut got_by_snp: HashMap<String, &mr::harmonise::HarmoniseOutput> = HashMap::new();
    for g in &got {
        got_by_snp.insert(g.snp.clone(), g);
    }

    let mut mismatches = 0;
    for e in &expected {
        let g = match got_by_snp.get(&e[0]) {
            Some(g) => *g,
            None => {
                eprintln!("SNP missing in Rust output: {}", e[0]);
                mismatches += 1;
                continue;
            }
        };
        let exp_b_exp: f64 = e[1].parse().unwrap();
        let exp_b_out: f64 = e[2].parse().unwrap();
        let exp_eaf_exp = opt_f64(&e[3]);
        let exp_eaf_out = opt_f64(&e[4]);
        let exp_ea_exp = &e[5];
        let exp_ea_out = &e[7];
        let exp_keep = e[9] == "TRUE";

        let mut bad = false;
        if (g.beta_exposure - exp_b_exp).abs() > 1e-9 {
            bad = true;
        }
        if (g.beta_outcome - exp_b_out).abs() > 1e-9 {
            bad = true;
        }
        if g.effect_allele_exposure != *exp_ea_exp {
            bad = true;
        }
        if g.effect_allele_outcome != *exp_ea_out {
            bad = true;
        }
        if g.mr_keep != exp_keep {
            bad = true;
        }
        if let (Some(a), Some(b)) = (g.eaf_exposure, exp_eaf_exp) {
            if (a - b).abs() > 1e-9 {
                bad = true;
            }
        }
        if let (Some(a), Some(b)) = (g.eaf_outcome, exp_eaf_out) {
            if (a - b).abs() > 1e-9 {
                bad = true;
            }
        }
        if bad {
            mismatches += 1;
            if mismatches <= 5 {
                eprintln!(
                    "MISMATCH {}: b_exp {}/{} b_out {}/{} ea_exp {}/{} ea_out {}/{} keep {}/{}",
                    e[0],
                    g.beta_exposure,
                    exp_b_exp,
                    g.beta_outcome,
                    exp_b_out,
                    g.effect_allele_exposure,
                    exp_ea_exp,
                    g.effect_allele_outcome,
                    exp_ea_out,
                    g.mr_keep,
                    exp_keep
                );
            }
        }
    }
    assert_eq!(mismatches, 0, "{mismatches} SNP rows differ from R");
    assert_eq!(got.len(), expected.len(), "row count differs");
}
