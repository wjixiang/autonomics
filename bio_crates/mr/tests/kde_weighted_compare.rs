//! Point-by-point comparison of our `density` against R's `stats::density`
//! output (dumped to `tests/data/r_density_weighted.tsv`) on the real
//! weighted-mode inputs. Diagnostic — not part of the golden suite.

fn split_csv(line: &str) -> Vec<String> {
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

fn load_beta_iv() -> (Vec<f64>, Vec<f64>) {
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/commondata_kept.csv"
    ))
    .unwrap();
    let mut bx = Vec::new();
    let mut bo = Vec::new();
    let mut sx = Vec::new();
    let mut so = Vec::new();
    for line in csv.lines().skip(1) {
        let f = split_csv(line);
        bx.push(f[3].parse::<f64>().unwrap());
        bo.push(f[4].parse::<f64>().unwrap());
        sx.push(f[5].parse::<f64>().unwrap());
        so.push(f[6].parse::<f64>().unwrap());
    }
    let n = bx.len();
    let beta_iv: Vec<f64> = (0..n).map(|i| bo[i] / bx[i]).collect();
    let se1: Vec<f64> = (0..n)
        .map(|i| {
            let be2 = bx[i] * bx[i];
            (so[i] * so[i] / be2 + bo[i] * bo[i] * sx[i] * sx[i] / (be2 * be2)).sqrt()
        })
        .collect();
    (beta_iv, se1)
}

#[test]
#[allow(clippy::needless_range_loop)]
fn compare_density_to_r() {
    let (beta_iv, se1) = load_beta_iv();
    let n = beta_iv.len();
    // h = 0.9 * min(sd,mad) / n^0.2  (R reported h = 0.1761936)
    let mean = beta_iv.iter().sum::<f64>() / n as f64;
    let sd = (beta_iv.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)).sqrt();
    let mut s = beta_iv.clone();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    };
    let mut dev: Vec<f64> = beta_iv.iter().map(|v| (v - med).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mad = (if n % 2 == 1 {
        dev[n / 2]
    } else {
        0.5 * (dev[n / 2 - 1] + dev[n / 2])
    }) * 1.4826;
    let h = 0.9 * sd.min(mad) / (n as f64).powf(0.2);
    println!("Rust: sd={sd} mad={mad} h={h}");

    // weights = 1/se1^2 (density normalises internally).
    let w: Vec<f64> = se1.iter().map(|s| 1.0 / (s * s)).collect();
    let d = mr::kde::density(&beta_iv, &w, h);
    println!("Rust argmax_x = {}", d.argmax_x());

    // Load R's dumped (x, y).
    let tsv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/r_density_weighted.tsv"
    ))
    .unwrap();
    let mut rx = Vec::new();
    let mut ry = Vec::new();
    for line in tsv.lines() {
        let mut it = line.split('\t');
        rx.push(it.next().unwrap().parse::<f64>().unwrap());
        ry.push(it.next().unwrap().parse::<f64>().unwrap());
    }
    let r_argmax = rx[ry
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .unwrap()
        .0];
    println!("R    argmax_x = {r_argmax}");

    // Compare on shared grid (x values should match).
    let mut max_diff = 0.0f64;
    let mut max_diff_at = 0.0f64;
    let n = d.y.len().min(ry.len());
    for i in 0..n {
        let diff = (d.y[i] - ry[i]).abs();
        if diff > max_diff {
            max_diff = diff;
            max_diff_at = d.x[i];
        }
    }
    println!("max |y_rust - y_R| = {max_diff}  at x={max_diff_at}");
    // Argmax must match R exactly (this is what the mode estimator reads).
    assert!(
        (d.argmax_x() - r_argmax).abs() < 1e-9,
        "argmax {} != R {}",
        d.argmax_x(),
        r_argmax
    );
    // y vector matches R to within the massdist-binning discrepancy.
    assert!(max_diff < 1e-3, "max |y_rust - y_R| = {max_diff}");
}
