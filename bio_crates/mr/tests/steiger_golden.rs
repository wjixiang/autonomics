//! Golden validation of `mr_steiger` against R (`R/steiger.R` + `psych::r.test`)
//! on the 79-SNP commondata fixture.

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

#[test]
fn steiger_matches_r() {
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/commondata_kept.csv"
    ))
    .unwrap();
    let mut p_exp = Vec::new();
    let mut p_out = Vec::new();
    let mut n_exp = Vec::new();
    let mut n_out = Vec::new();
    for line in csv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f = split(line);
        // SNP, id.exp, id.out, beta.exp, beta.out, se.exp, se.out, mr_keep,
        // pval.exposure(8), pval.outcome(9), samplesize.exposure(10),
        // samplesize.outcome(11), eaf.exp, eaf.out
        p_exp.push(f[8].parse::<f64>().unwrap());
        p_out.push(f[9].parse::<f64>().unwrap());
        n_exp.push(f[10].parse::<f64>().unwrap());
        n_out.push(f[11].parse::<f64>().unwrap());
    }
    let na = vec![f64::NAN; p_exp.len()];
    let r = mr::steiger::mr_steiger(&p_exp, &p_out, &n_exp, &n_out, &na, &na, 1.0, 1.0);

    // R golden: r2_exp=0.01580819, r2_out=0.001350485, correct_dir=TRUE,
    // steiger_pval=1.748577e-207, sensitivity_ratio=7.735403.
    assert!((r.r2_exp - 0.01580819).abs() < 1e-6, "r2_exp={}", r.r2_exp);
    assert!((r.r2_out - 0.001350485).abs() < 1e-7, "r2_out={}", r.r2_out);
    assert!(r.correct_causal_direction);
    assert!(
        (r.steiger_test - 1.748577e-207).abs() < 1e-210,
        "steiger_pval={}",
        r.steiger_test
    );
    assert!(
        (r.sensitivity_ratio - 7.735403).abs() < 1e-3,
        "sens={}",
        r.sensitivity_ratio
    );
}
