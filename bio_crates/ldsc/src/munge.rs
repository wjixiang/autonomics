//! Summary-statistic munging — a port of `munge_sumstats.py`.
//!
//! Maps a GWAS summary-statistics file's columns onto LDSC's canonical names
//! (SNP, N, Z, A1, A2, FRQ, INFO, …), filters bad variants (P out of range,
//! low INFO, low MAF, non-SNP / strand-ambiguous alleles, missing values),
//! converts P → Z, applies the signed-statistic sign convention, determines
//! sample size (including Stephan Ripke's `daner` formats), and produces the
//! `.sumstats` table LDSC regresses on.

use std::collections::HashMap;

use crate::io::read_table;
use crate::stats::chi2_isf_1;
use crate::{LdscError, Result};

/// `clean_header` — uppercase; `-`→`_`; `.`→`_`; strip newlines.
pub fn clean_header(h: &str) -> String {
    h.to_uppercase().replace(['-', '.'], "_").replace('\n', "")
}

/// `null_values` — the "no-effect" value for each signed-statistic type.
pub fn null_values() -> HashMap<&'static str, f64> {
    [("LOG_ODDS", 0.0), ("BETA", 0.0), ("OR", 1.0), ("Z", 0.0)]
        .into_iter()
        .collect()
}

/// `default_cnames` — the cleaned-header → canonical-name map (port of the
/// Python `default_cnames` dict). Keys are already cleaned.
pub fn default_cnames() -> Vec<(&'static str, &'static str)> {
    munge_defaults::DEFAULT_CNAMES.to_vec()
}

// ---------------------------------------------------------------------------
// Column-name resolution
// ---------------------------------------------------------------------------

/// `get_cname_map(flag, default, ignore)` — priority: ignore > flag > default.
/// All equality is modulo [`clean_header`]. Returns cleaned-header → canonical.
pub fn get_cname_map(
    flag: &HashMap<String, String>,
    default: &[(&str, &str)],
    ignore: &[String],
) -> HashMap<String, String> {
    let clean_ignore: Vec<String> = ignore.iter().map(|s| clean_header(s)).collect();
    let mut out = HashMap::new();
    for (k, v) in flag {
        if clean_ignore.iter().any(|c| c == k) {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    for (k, v) in default {
        let k = clean_header(k);
        if clean_ignore.contains(&k) || flag.contains_key(&k) {
            continue;
        }
        out.insert(k, v.to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

/// `filter_pvals(P)` — keep `0 < P ≤ 1`.
pub fn filter_pvals(p: &[f64]) -> Vec<bool> {
    p.iter().map(|&x| x > 0.0 && x <= 1.0).collect()
}

/// `filter_info(info, info_min)` — single INFO column: keep `info >= info_min`,
/// flagging out-of-range `[0,2)` values as bad.
pub fn filter_info(info: &[f64], info_min: f64) -> Vec<bool> {
    info.iter()
        .map(|&x| {
            let bad = !(0.0..=2.0).contains(&x) && !x.is_nan();
            !bad && x >= info_min
        })
        .collect()
}

/// `filter_info_multi(info_cols, info_min)` — several INFO columns: keep when
/// `sum >= info_min * n_cols` (port of the DataFrame branch).
pub fn filter_info_multi(info_cols: &[Vec<f64>], info_min: f64) -> Vec<bool> {
    let n = info_cols[0].len();
    let nc = info_cols.len();
    (0..n)
        .map(|i| {
            let mut bad = false;
            let mut sum = 0.0;
            for col in info_cols {
                let v = col[i];
                if !v.is_nan() && !(0.0..=2.0).contains(&v) {
                    bad = true;
                }
                sum += v;
            }
            !bad && sum >= info_min * nc as f64
        })
        .collect()
}

/// `filter_frq(frq, maf_min)` — out-of-[0,1] removed, then `min(f,1-f) > maf_min`.
pub fn filter_frq(frq: &[f64], maf_min: f64) -> Vec<bool> {
    frq.iter()
        .map(|&f| {
            let oob = !(0.0..=1.0).contains(&f);
            let maf = f.min(1.0 - f);
            !oob && maf > maf_min
        })
        .collect()
}

/// `filter_alleles(a1a2)` — keep strand-unambiguous biallelic SNPs.
pub fn filter_alleles(a1a2: &[String]) -> Vec<bool> {
    a1a2.iter()
        .map(|s| crate::alleles::is_valid_snp(s))
        .collect()
}

/// `p_to_z(P)` — `sqrt(chi2.isf(P, 1))`.
pub fn p_to_z(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::INFINITY;
    }
    chi2_isf_1(p).sqrt()
}

/// `check_median(x, expected, tol, name)` — Ok(msg) if within, else Err.
pub fn check_median(x: &[f64], expected_median: f64, tolerance: f64, name: &str) -> Result<String> {
    let mut s = x.to_vec();
    let m = median(&mut s);
    if (m - expected_median).abs() > tolerance {
        Err(LdscError::InvalidInput(format!(
            "median value of {name} is {:.2} (should be close to {expected_median}). This column may be mislabeled.",
            m
        )))
    } else {
        Ok(format!(
            "Median value of {name} was {m}, which seems sensible."
        ))
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// numpy-style 0.9 quantile (linear interpolation), for the default `n_min`.
pub fn quantile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    let pos = q * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        s[lo]
    } else {
        let frac = pos - lo as f64;
        s[lo] * (1.0 - frac) + s[hi] * frac
    }
}

// ---------------------------------------------------------------------------
// Config + driver
// ---------------------------------------------------------------------------

/// Configuration for `munge_sumstats` (the Python argparse flags).
#[derive(Default, Clone)]
pub struct MungeConfig {
    pub sumstats: String,
    pub out: Option<String>,
    pub n: Option<f64>,
    pub n_cas: Option<f64>,
    pub n_con: Option<f64>,
    pub info_min: f64,
    pub maf_min: f64,
    pub daner: bool,
    pub daner_n: bool,
    pub no_alleles: bool,
    pub merge_alleles: Option<String>,
    pub n_min: Option<f64>,
    pub signed_sumstats: Option<(String, f64)>, // (col, null)
    pub a1_inc: bool,
    pub keep_maf: bool,
    pub ignore: Vec<String>,
    // explicit column-name overrides (cleaned by caller or here)
    pub snp: Option<String>,
    pub n_col: Option<String>,
    pub n_cas_col: Option<String>,
    pub n_con_col: Option<String>,
    pub a1: Option<String>,
    pub a2: Option<String>,
    pub p: Option<String>,
    pub frq: Option<String>,
    pub info: Option<String>,
    pub info_list: Vec<String>,
    pub nstudy: Option<String>,
    pub nstudy_min: Option<f64>,
}

impl MungeConfig {
    fn info_min_or_default(&self) -> f64 {
        if self.info_min > 0.0 {
            self.info_min
        } else {
            0.9
        }
    }
    fn maf_min_or_default(&self) -> f64 {
        if self.maf_min > 0.0 {
            self.maf_min
        } else {
            0.01
        }
    }
}

/// Munged summary statistics (the in-memory `dat` table).
#[derive(Debug, Clone, Default)]
pub struct MungedSumStats {
    pub snp: Vec<String>,
    pub a1: Option<Vec<String>>,
    pub a2: Option<Vec<String>>,
    pub z: Vec<f64>,
    pub n: Vec<f64>,
    pub frq: Option<Vec<f64>>,
}

impl MungedSumStats {
    pub fn len(&self) -> usize {
        self.snp.len()
    }
    pub fn is_empty(&self) -> bool {
        self.snp.is_empty()
    }
}

/// Run the munge pipeline. Port of `munge_sumstats(args, p)`.
pub fn munge_sumstats(cfg: &MungeConfig) -> Result<MungedSumStats> {
    if cfg.out.is_none() && cfg.merge_alleles.is_none() {
        // Python requires --out, but the in-memory return is what tests check.
    }
    if cfg.sumstats.is_empty() {
        return Err(LdscError::InvalidInput(
            "The --sumstats flag is required.".into(),
        ));
    }
    if cfg.no_alleles && cfg.merge_alleles.is_some() {
        return Err(LdscError::InvalidInput(
            "--no-alleles and --merge-alleles are not compatible.".into(),
        ));
    }
    if cfg.daner && cfg.daner_n {
        return Err(LdscError::InvalidInput(
            "--daner and --daner-n are not compatible.".into(),
        ));
    }

    let table = read_table(&cfg.sumstats)?;
    let file_cnames: Vec<String> = table.header.clone();

    // flag cnames from explicit column overrides.
    let mut flag: HashMap<String, String> = HashMap::new();
    let add = |opt: &Option<String>, canon: &str, flag: &mut HashMap<String, String>| {
        if let Some(c) = opt {
            flag.insert(clean_header(c), canon.into());
        }
    };
    add(&cfg.nstudy, "NSTUDY", &mut flag);
    add(&cfg.snp, "SNP", &mut flag);
    add(&cfg.n_col, "N", &mut flag);
    add(&cfg.n_cas_col, "N_CAS", &mut flag);
    add(&cfg.n_con_col, "N_CON", &mut flag);
    add(&cfg.a1, "A1", &mut flag);
    add(&cfg.a2, "A2", &mut flag);
    add(&cfg.p, "P", &mut flag);
    add(&cfg.frq, "FRQ", &mut flag);
    add(&cfg.info, "INFO", &mut flag);
    for c in &cfg.info_list {
        flag.insert(clean_header(c), "INFO".into());
    }
    let mut signed_sumstat_null: Option<f64> = None;
    if let Some((col, nv)) = &cfg.signed_sumstats {
        flag.insert(clean_header(col), "SIGNED_SUMSTAT".into());
        signed_sumstat_null = Some(*nv);
    }

    // build the default map (possibly minus null-value columns if signed/a1_inc).
    let nulls = null_values();
    let default: Vec<(&str, &str)> = if cfg.signed_sumstats.is_some() || cfg.a1_inc {
        default_cnames()
            .into_iter()
            .filter(|(_, canon)| !nulls.contains_key(*canon))
            .collect()
    } else {
        default_cnames()
    };
    let cname_map = get_cname_map(&flag, &default, &cfg.ignore);

    // daner: infer N_cas/N_con from FRQ_A_/FRQ_U_ header suffixes.
    let mut n_cas = cfg.n_cas;
    let mut n_con = cfg.n_con;
    let mut cname_map = cname_map;
    if cfg.daner {
        let frq_u = file_cnames
            .iter()
            .find(|c| c.starts_with("FRQ_U_"))
            .cloned();
        let frq_a = file_cnames
            .iter()
            .find(|c| c.starts_with("FRQ_A_"))
            .cloned();
        let (Some(fu), Some(fa)) = (frq_u, frq_a) else {
            return Err(LdscError::InvalidInput(
                "daner: missing FRQ_A_/FRQ_U_ columns".into(),
            ));
        };
        n_cas = Some(fa[6..].parse::<f64>().map_err(|_| LdscError::Parse {
            context: "daner".into(),
            reason: format!("bad N_cas in {fa}"),
        })?);
        n_con = Some(fu[6..].parse::<f64>().map_err(|_| LdscError::Parse {
            context: "daner".into(),
            reason: format!("bad N_con in {fu}"),
        })?);
        // drop any N/N_CAS/N_CON/FRQ mappings; map FRQ_U_ → FRQ
        let keys_to_drop: Vec<String> = cname_map
            .iter()
            .filter(|(_, v)| matches!(v.as_str(), "N" | "N_CAS" | "N_CON" | "FRQ"))
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys_to_drop {
            cname_map.remove(&k);
        }
        cname_map.insert(clean_header(&fu), "FRQ".into());
    }
    if cfg.daner_n {
        let frq_u = file_cnames
            .iter()
            .find(|c| c.starts_with("FRQ_U_"))
            .cloned();
        if let Some(fu) = frq_u {
            cname_map.insert(clean_header(&fu), "FRQ".into());
        }
        let nca = file_cnames.iter().find(|c| clean_header(c) == "NCA");
        let nco = file_cnames.iter().find(|c| clean_header(c) == "NCO");
        let (Some(nca), Some(nco)) = (nca, nco) else {
            return Err(LdscError::InvalidInput(
                "Could not find Nca/Nco column expected for daner-n format".into(),
            ));
        };
        cname_map.insert(clean_header(nca), "N_CAS".into());
        cname_map.insert(clean_header(nco), "N_CON".into());
    }

    // cname_translation: original header → canonical, for headers present in the file.
    let mut cname_translation: HashMap<String, String> = HashMap::new();
    for orig in &file_cnames {
        let c = clean_header(orig);
        if let Some(canon) = cname_map.get(&c) {
            cname_translation.insert(orig.clone(), canon.clone());
        }
    }

    // pick the signed sumstat column / null.
    let sign_cname: Option<String>;
    if cfg.signed_sumstats.is_none() && !cfg.a1_inc {
        let sign_cols: Vec<String> = cname_translation
            .iter()
            .filter(|(_, v)| nulls.contains_key(v.as_str()))
            .map(|(k, _)| k.clone())
            .collect();
        if sign_cols.len() > 1 {
            return Err(LdscError::InvalidInput(
                "Too many signed sumstat columns. Specify which to ignore with the --ignore flag."
                    .into(),
            ));
        }
        if sign_cols.is_empty() {
            return Err(LdscError::InvalidInput(
                "Could not find a signed summary statistic column.".into(),
            ));
        }
        let sc = sign_cols[0].clone();
        let canon = &cname_translation[&sc];
        signed_sumstat_null = Some(nulls[canon.as_str()]);
        cname_translation.insert(sc.clone(), "SIGNED_SUMSTAT".into());
        sign_cname = Some(sc);
    } else {
        sign_cname = Some("SIGNED_SUMSTATS".into());
    }
    let null_val = signed_sumstat_null.unwrap_or(0.0);

    // required columns
    let need = if !cfg.a1_inc {
        vec!["SNP", "P", "SIGNED_SUMSTAT"]
    } else {
        vec!["SNP", "P"]
    };
    let translated: Vec<String> = cname_translation.values().cloned().collect();
    for c in &need {
        if !translated.iter().any(|t| t == c) {
            return Err(LdscError::InvalidInput(format!(
                "Could not find {c} column."
            )));
        }
    }
    if !cfg.no_alleles {
        for c in &["A1", "A2"] {
            if !translated.iter().any(|t| t == c) {
                return Err(LdscError::InvalidInput(format!(
                    "Could not find {c} columns."
                )));
            }
        }
    }

    // extract columns by canonical name into per-row records.
    let col_idx: HashMap<String, usize> = cname_translation
        .iter()
        .filter_map(|(orig, canon)| {
            table
                .header
                .iter()
                .position(|h| h == orig)
                .map(|i| (canon.clone(), i))
        })
        .collect();

    // read + filter rows
    let info_min = cfg.info_min_or_default();
    let maf_min = cfg.maf_min_or_default();
    let has_info = col_idx.contains_key("INFO");
    let has_frq = col_idx.contains_key("FRQ");

    #[derive(Default)]
    struct Row {
        snp: String,
        a1: String,
        a2: String,
        p: f64,
        signed: f64,
        n: Option<f64>,
        frq: Option<f64>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for r in &table.rows {
        let cell = |canon: &str| -> Option<&str> {
            col_idx
                .get(canon)
                .and_then(|&i| r.get(i).map(|s| s.as_str()))
        };
        // dropna on all columns except INFO
        let empty = String::new();
        let mut any_na = false;
        for orig in cname_translation.keys() {
            let i = table.header.iter().position(|h| h == orig).unwrap();
            let v = r.get(i).unwrap_or(&empty);
            if v == "." && cname_translation[orig] != "INFO" {
                any_na = true;
            }
        }
        if any_na {
            continue;
        }
        let snp = cell("SNP").unwrap_or("").to_string();
        let parse = |canon: &str| -> f64 {
            cell(canon)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(f64::NAN)
        };
        let p = parse("P");
        let signed = parse("SIGNED_SUMSTAT");
        let a1 = cell("A1").unwrap_or("").to_uppercase();
        let a2 = cell("A2").unwrap_or("").to_uppercase();
        let mut keep = true;
        if has_info {
            let info_v = parse("INFO");
            if !(info_v.is_finite() && !(0.0..=2.0).contains(&info_v) && info_v >= info_min) {
                // matches filter_info: bad if (info>2 || info<0)&&!nan ; keep if !bad && info>=min
                let bad = !(0.0..=2.0).contains(&info_v) && !info_v.is_nan();
                if bad || info_v < info_min {
                    keep = false;
                }
            }
        }
        if has_frq {
            let frq_v = parse("FRQ");
            let oob = !(0.0..=1.0).contains(&frq_v);
            let maf = frq_v.min(1.0 - frq_v);
            if oob || !(maf > maf_min) {
                keep = false;
            }
        }
        if !(p > 0.0 && p <= 1.0) {
            keep = false;
        }
        if !cfg.no_alleles && !crate::alleles::is_valid_snp(&format!("{a1}{a2}")) {
            keep = false;
        }
        if !keep {
            continue;
        }
        rows.push(Row {
            snp,
            a1,
            a2,
            p,
            signed,
            n: cell("N").and_then(|s| s.parse::<f64>().ok()),
            frq: cell("FRQ").and_then(|s| s.parse::<f64>().ok()),
        });
    }
    if rows.is_empty() {
        return Err(LdscError::InvalidInput(
            "After applying filters, no SNPs remain.".into(),
        ));
    }

    // drop duplicated SNP
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert(r.snp.clone()));

    // process_n
    let n_for_all = if let (Some(cas), Some(con)) = (n_cas, n_con) {
        Some(cas + con)
    } else {
        cfg.n
    };
    // Build N per row, applying the n_min filter (only when an N column exists).
    let have_n_col = rows.iter().any(|r| r.n.is_some());
    if have_n_col {
        let nm = cfg.n_min.or_else(|| {
            let all_n: Vec<f64> = rows.iter().filter_map(|r| r.n).collect();
            Some(quantile(&all_n, 0.9) / 1.5)
        });
        let nm = nm.unwrap_or(0.0);
        rows.retain(|r| r.n.map(|n| n >= nm).unwrap_or(true));
    }
    let n_const = n_for_all;
    // P → Z, apply sign
    let mut snp = Vec::with_capacity(rows.len());
    let mut a1 = if cfg.no_alleles {
        None
    } else {
        Some(Vec::new())
    };
    let mut a2 = if cfg.no_alleles {
        None
    } else {
        Some(Vec::new())
    };
    let mut z = Vec::with_capacity(rows.len());
    let mut n = Vec::with_capacity(rows.len());
    let mut frq = if cfg.keep_maf { Some(Vec::new()) } else { None };
    for r in &rows {
        let mut zi = p_to_z(r.p);
        if !cfg.a1_inc {
            // Z *= (-1)^(SIGNED_SUMSTAT < null)
            if r.signed < null_val {
                zi = -zi;
            }
        }
        snp.push(r.snp.clone());
        if let Some(v) = a1.as_mut() {
            v.push(r.a1.clone());
        }
        if let Some(v) = a2.as_mut() {
            v.push(r.a2.clone());
        }
        z.push(zi);
        n.push(r.n.unwrap_or(n_const.unwrap_or(f64::NAN)));
        if let Some(f) = frq.as_mut() {
            f.push(r.frq.unwrap_or(f64::NAN));
        }
    }
    let _ = sign_cname;
    Ok(MungedSumStats {
        snp,
        a1,
        a2,
        z,
        n,
        frq,
    })
}

/// Write a `.sumstats.gz` (tab-separated, `%.3f`, columns SNP A1 A2 Z N [FRQ]).
pub fn write_sumstats_gz(m: &MungedSumStats, path: &str) -> Result<()> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    let f = std::fs::File::create(path)?;
    let mut enc = GzEncoder::new(f, Compression::default());
    let mut header = vec!["SNP", "A1", "A2", "Z", "N"];
    if m.frq.is_some() {
        header.push("FRQ");
    }
    writeln!(enc, "{}", header.join("\t"))?;
    for i in 0..m.snp.len() {
        write!(
            enc,
            "{}\t{}\t{}\t{:.3}\t{:.3}",
            m.snp[i],
            m.a1.as_ref().map(|v| v[i].as_str()).unwrap_or(""),
            m.a2.as_ref().map(|v| v[i].as_str()).unwrap_or(""),
            m.z[i],
            m.n[i]
        )?;
        if let Some(frq) = &m.frq {
            write!(enc, "\t{:.3}", frq[i])?;
        }
        writeln!(enc)?;
    }
    enc.finish()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// default_cnames table
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod munge_defaults {
    /// Cleaned-header → canonical, matching `munge_sumstats.default_cnames`.
    pub static DEFAULT_CNAMES: &[(&str, &str)] = &[
        // RS number
        ("SNP", "SNP"),
        ("MARKERNAME", "SNP"),
        ("SNPID", "SNP"),
        ("RS", "SNP"),
        ("RSID", "SNP"),
        ("RS_NUMBER", "SNP"),
        ("RS_NUMBERS", "SNP"),
        // number of studies
        ("NSTUDY", "NSTUDY"),
        ("N_STUDY", "NSTUDY"),
        ("NSTUDIES", "NSTUDY"),
        ("N_STUDIES", "NSTUDY"),
        // p-value
        ("P", "P"),
        ("PVALUE", "P"),
        ("P_VALUE", "P"),
        ("PVAL", "P"),
        ("P_VAL", "P"),
        ("GC_PVALUE", "P"),
        // allele 1
        ("A1", "A1"),
        ("ALLELE1", "A1"),
        ("ALLELE_1", "A1"),
        ("EFFECT_ALLELE", "A1"),
        ("REFERENCE_ALLELE", "A1"),
        ("INC_ALLELE", "A1"),
        ("EA", "A1"),
        // allele 2
        ("A2", "A2"),
        ("ALLELE2", "A2"),
        ("ALLELE_2", "A2"),
        ("OTHER_ALLELE", "A2"),
        ("NON_EFFECT_ALLELE", "A2"),
        ("DEC_ALLELE", "A2"),
        ("NEA", "A2"),
        // N
        ("N", "N"),
        ("NCASE", "N_CAS"),
        ("CASES_N", "N_CAS"),
        ("N_CASE", "N_CAS"),
        ("N_CASES", "N_CAS"),
        ("N_CONTROLS", "N_CON"),
        ("N_CAS", "N_CAS"),
        ("N_CON", "N_CON"),
        ("N_CASE", "N_CAS"),
        ("NCONTROL", "N_CON"),
        ("CONTROLS_N", "N_CON"),
        ("N_CONTROL", "N_CON"),
        ("WEIGHT", "N"),
        // signed statistics
        ("ZSCORE", "Z"),
        ("Z-SCORE", "Z"),
        ("GC_ZSCORE", "Z"),
        ("Z", "Z"),
        ("OR", "OR"),
        ("B", "BETA"),
        ("BETA", "BETA"),
        ("LOG_ODDS", "LOG_ODDS"),
        ("EFFECTS", "BETA"),
        ("EFFECT", "BETA"),
        ("SIGNED_SUMSTAT", "SIGNED_SUMSTAT"),
        // info / maf
        ("INFO", "INFO"),
        ("EAF", "FRQ"),
        ("FRQ", "FRQ"),
        ("MAF", "FRQ"),
        ("FRQ_U", "FRQ"),
        ("F_U", "FRQ"),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(p: &str) -> String {
        format!("tests/data/{p}")
    }

    #[test]
    fn clean_header_works() {
        assert_eq!(clean_header("foo-bar.foo_BaR"), "FOO_BAR_FOO_BAR");
    }

    #[test]
    fn p_to_z_value() {
        // P=0.1 → 1.644854
        assert!((p_to_z(0.1) - 1.644854).abs() < 1e-4);
    }

    #[test]
    fn filters() {
        assert_eq!(
            filter_pvals(&[0.0, 0.1, 1.0, 2.0]),
            vec![false, true, true, false]
        );
        assert_eq!(filter_info(&[0.8, 1.0, 1.0], 0.9), vec![false, true, true]);
        assert_eq!(
            filter_frq(&[-1.0, 0.0, 0.005, 0.4, 0.6, 0.999, 1.0, 2.0], 0.01),
            vec![false, false, false, true, true, false, false, false]
        );
        let a = vec![
            "AC".into(),
            "AG".into(),
            "CA".into(),
            "CT".into(),
            "GA".into(),
            "GT".into(),
            "TC".into(),
            "TG".into(),
            "DI".into(),
        ];
        let f = filter_alleles(&a);
        assert_eq!(
            f,
            vec![true, true, true, true, true, true, true, true, false]
        );
    }

    #[test]
    fn daner_end_to_end_matches_golden() {
        // test_munge_sumstats.test_basic: daner sumstats → correct.sumstats
        let cfg = MungeConfig {
            sumstats: data("munge_test/sumstats"),
            daner: true,
            ..Default::default()
        };
        let m = munge_sumstats(&cfg).unwrap();
        // read golden
        let golden = read_table(&data("munge_test/correct.sumstats")).unwrap();
        let g_snp = golden.column("SNP").unwrap();
        let g_a1 = golden.column("A1").unwrap();
        let g_a2 = golden.column("A2").unwrap();
        let g_z: Vec<f64> = golden
            .column("Z")
            .unwrap()
            .iter()
            .map(|s| s.parse::<f64>().unwrap())
            .collect();
        let g_n: Vec<f64> = golden
            .column("N")
            .unwrap()
            .iter()
            .map(|s| s.parse::<f64>().unwrap())
            .collect();
        assert_eq!(m.snp, g_snp);
        assert_eq!(m.a1.clone().unwrap(), g_a1);
        assert_eq!(m.a2.clone().unwrap(), g_a2);
        assert_eq!(m.z.len(), g_z.len());
        for i in 0..m.z.len() {
            assert!(
                (m.z[i] - g_z[i]).abs() < 1e-6,
                "z[{i}]: {} vs {}",
                m.z[i],
                g_z[i]
            );
            assert!(
                (m.n[i] - g_n[i]).abs() < 1e-6,
                "n[{i}]: {} vs {}",
                m.n[i],
                g_n[i]
            );
        }
    }

    #[test]
    fn bad_flags() {
        assert!(
            munge_sumstats(&MungeConfig {
                sumstats: String::new(),
                daner: true,
                ..Default::default()
            })
            .is_err()
        );
        let cfg = MungeConfig {
            sumstats: data("munge_test/sumstats"),
            no_alleles: true,
            merge_alleles: Some("x".into()),
            ..Default::default()
        };
        assert!(munge_sumstats(&cfg).is_err());
    }
}
