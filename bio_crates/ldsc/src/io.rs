//! LDSC file-format readers — a faithful port of `ldscore/parse.py`.
//!
//! Everything here is "no math, just I/O": reading the various whitespace- and
//! tab-delimited tables LDSC uses (LD Scores, `M`, annotations, frequencies,
//! summary statistics, PLINK `.bim`/`.fam`, filter files), transparently
//! decompressing `.gz` / `.bz2` / plain files, and resolving per-chromosome
//! filesets.
//!
//! Pandas reads these with `delim_whitespace=True` (splits on runs of any
//! whitespace). That is *not* expressible with a single-delimiter CSV parser,
//! so we tokenize each line with `str::split_whitespace` — which matches
//! pandas exactly and adds no dependency.
//!
//! All numerics use `f64`. Missing values follow pandas `na_values='.'`
//! (the token `.` → `NaN`).

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;

use crate::{LdscError, Result};

/// Number of autosomes LDSC splits files across.
pub const N_CHR: usize = 22;

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

/// Compression inferred from a filename suffix (`get_compression`) or by
/// probing the filesystem (`which_compression`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Bzip2,
}

/// `get_compression(fh)` — suffix-based: `.gz`→Gzip, `.bz2`→Bzip2, else None.
pub fn get_compression(fh: &str) -> Compression {
    if fh.ends_with("gz") {
        Compression::Gzip
    } else if fh.ends_with("bz2") {
        Compression::Bzip2
    } else {
        Compression::None
    }
}

/// `which_compression(fh)` — probe `fh + ".bz2"`, `fh + ".gz"`, then `fh`.
/// Returns the suffix that matched (`.bz2` / `.gz` / `""`) and its compression.
pub fn which_compression(base: &str) -> Result<(String, Compression)> {
    let bz2 = format!("{base}.bz2");
    if Path::new(&bz2).exists() {
        return Ok((".bz2".to_string(), Compression::Bzip2));
    }
    let gz = format!("{base}.gz");
    if Path::new(&gz).exists() {
        return Ok((".gz".to_string(), Compression::Gzip));
    }
    if Path::new(base).exists() {
        return Ok((String::new(), Compression::None));
    }
    Err(LdscError::Compression(format!(
        "Could not open {base}[./gz/bz2]"
    )))
}

/// Open `path` for reading, decompressing by suffix. Returns a boxed reader.
fn open_reader(path: &str) -> Result<Box<dyn Read>> {
    let file = File::open(path).map_err(LdscError::Io)?;
    Ok(match get_compression(path) {
        Compression::None => Box::new(BufReader::new(file)),
        Compression::Gzip => Box::new(BufReader::new(GzDecoder::new(file))),
        Compression::Bzip2 => Box::new(BufReader::new(bzip2_rs::DecoderReader::new(file))),
    })
}

/// Read every line of `path` (decompressing by suffix), trimming trailing
/// newlines but keeping blank lines (callers skip them).
pub fn read_lines(path: &str) -> Result<Vec<String>> {
    let reader = open_reader(path)?;
    let buf = BufReader::new(reader);
    let mut out = Vec::new();
    for line in buf.lines() {
        let line = line?;
        out.push(line);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Generic whitespace-delimited table
// ---------------------------------------------------------------------------

/// A whitespace-delimited table: a header row plus data rows (all string cells).
#[derive(Debug, Clone)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn col_idx(&self, name: &str) -> Result<usize> {
        self.header
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| LdscError::Parse {
                context: "table".into(),
                reason: format!("column '{name}' not found"),
            })
    }

    /// Owned column of raw string cells.
    pub fn column(&self, name: &str) -> Result<Vec<String>> {
        let j = self.col_idx(name)?;
        Ok(self
            .rows
            .iter()
            .map(|r| r.get(j).cloned().unwrap_or_default())
            .collect())
    }
}

/// Read a whitespace-delimited table where the first non-empty line is the
/// header. Blank lines are skipped. Port of `parse.read_csv(header=0, ...)`.
pub fn read_table(path: &str) -> Result<Table> {
    let lines = read_lines(path)?;
    parse_table_lines(&lines)
}

fn parse_table_lines(lines: &[String]) -> Result<Table> {
    let mut iter = lines.iter().filter(|l| !l.trim().is_empty());
    let header_line = iter.next().ok_or_else(|| LdscError::Parse {
        context: "table".into(),
        reason: "empty file (no header)".into(),
    })?;
    let header: Vec<String> = header_line.split_whitespace().map(str::to_owned).collect();
    let mut rows = Vec::new();
    for line in iter {
        rows.push(line.split_whitespace().map(str::to_owned).collect());
    }
    Ok(Table { header, rows })
}

/// `series_eq(x, y)` — lengths equal and elementwise equal.
pub fn series_eq(x: &[String], y: &[String]) -> bool {
    x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| a == b)
}

// ---------------------------------------------------------------------------
// Cell coercion
// ---------------------------------------------------------------------------

/// Parse a cell as `f64`, treating the pandas NA token `.` as `NaN`.
fn parse_f64_cell(s: &str) -> f64 {
    if s == "." {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// Strict parse for `M` files: each token must be a real number. Python's
/// `[float(z) for z in ...]` raises on a non-numeric token (e.g. `.`).
fn parse_f64_strict(s: &str) -> Result<f64> {
    s.parse::<f64>().map_err(|_| LdscError::Parse {
        context: "M".into(),
        reason: format!("could not parse '{s}' as a number"),
    })
}

/// Is a string cell a pandas missing value (NA token `.`)?
fn is_na(s: &str) -> bool {
    s == "." || s.is_empty()
}

// ---------------------------------------------------------------------------
// Chromosome path helpers
// ---------------------------------------------------------------------------

/// `sub_chr(s, chrom)` — substitute `@` with the chromosome number, else append.
pub fn sub_chr(s: &str, chrom: u32) -> String {
    if s.contains('@') {
        s.replace('@', &chrom.to_string())
    } else {
        format!("{s}{chrom}")
    }
}

/// `get_present_chrs(fh, num)` — which chromosomes (1..num) have at least one
/// file matching `sub_chr(fh, chrom) + ".*"`.
pub fn get_present_chrs(prefix: &str, num: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for chrom in 1..num {
        let sub = sub_chr(prefix, chrom);
        if has_chr_files(&sub) {
            out.push(chrom);
        }
    }
    out
}

/// Does any entry in the parent directory start with `basename(sub) + "."`?
fn has_chr_files(sub: &str) -> bool {
    let path = Path::new(sub);
    let (dir, base) = match (path.parent(), path.file_name()) {
        (Some(d), Some(b)) if !d.as_os_str().is_empty() => (d, b.to_string_lossy().into_owned()),
        _ => (Path::new("."), path.to_string_lossy().into_owned()),
    };
    let prefix = format!("{base}.");
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// LD Score files  (parse.ldscore, ldscore_fromlist, l2_parser)
// ---------------------------------------------------------------------------

/// LD Scores for a set of SNPs. `colnames` are the LD-score column names
/// (excluding SNP); `cols[k]` is the column for `colnames[k]`, length `snp.len()`.
#[derive(Debug, Clone)]
pub struct LdScores {
    pub snp: Vec<String>,
    pub colnames: Vec<String>,
    pub cols: Vec<Vec<f64>>,
}

impl LdScores {
    pub fn n_rows(&self) -> usize {
        self.snp.len()
    }
}

/// `l2_parser` + `ldscore(fh, num)`: read one or per-chromosome `.l2.ldscore`
/// files, drop the geo columns (CHR/BP, plus CM/MAF when both present for
/// backwards compatibility), sort by CHR then BP, deduplicate by SNP (first
/// occurrence), and return the LD-score columns.
pub fn read_ldscore(prefix: &str, num: Option<u32>) -> Result<LdScores> {
    let mut records: Vec<(i64, i64, String, Vec<f64>)> = Vec::new();
    let mut ld_colnames: Vec<String> = Vec::new();

    let targets = resolve_ldscore_files(prefix, num)?;
    for (_chr, path) in &targets {
        let table = read_table(path)?;
        let chr_idx = table.col_idx("CHR")?;
        let bp_idx = table.col_idx("BP")?;
        let snp_idx = table.col_idx("SNP")?;
        // CM/MAF dropped iff both present (backwards-compat with v<1.0.0).
        let drop_cm_maf = table.col_idx("CM").is_ok() && table.col_idx("MAF").is_ok();
        let mut ld_idx = Vec::new();
        for (j, h) in table.header.iter().enumerate() {
            if j == chr_idx || j == bp_idx || j == snp_idx {
                continue;
            }
            if drop_cm_maf && (h == "CM" || h == "MAF") {
                continue;
            }
            ld_idx.push(j);
        }
        if ld_colnames.is_empty() {
            ld_colnames = ld_idx.iter().map(|&j| table.header[j].clone()).collect();
        }

        for row in &table.rows {
            let chr = row
                .get(chr_idx)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<i64>()
                .unwrap_or(0);
            let bp = row
                .get(bp_idx)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<i64>()
                .unwrap_or(0);
            let snp = row.get(snp_idx).cloned().unwrap_or_default();
            let ld: Vec<f64> = ld_idx
                .iter()
                .map(|&j| parse_f64_cell(row.get(j).map(|s| s.as_str()).unwrap_or(".")))
                .collect();
            records.push((chr, bp, snp, ld));
        }
    }

    if ld_colnames.is_empty() {
        return Err(LdscError::Parse {
            context: "ldscore".into(),
            reason: "no LD-score columns found".into(),
        });
    }

    // sort by (CHR, BP) — SEs are wrong unless sorted.
    records.sort_by_key(|a| (a.0, a.1));

    // dedup by SNP (first occurrence kept).
    let mut seen = HashSet::new();
    let mut snp_out = Vec::with_capacity(records.len());
    let n_ld = ld_colnames.len();
    let mut cols_out: Vec<Vec<f64>> = vec![Vec::new(); n_ld];
    for (_chr, _bp, snp, ld) in records {
        if !seen.insert(snp.clone()) {
            continue;
        }
        snp_out.push(snp);
        for k in 0..n_ld {
            cols_out[k].push(ld[k]);
        }
    }

    Ok(LdScores {
        snp: snp_out,
        colnames: ld_colnames,
        cols: cols_out,
    })
}

/// `ldscore_fromlist(flist)` — sideways concatenation of multiple LD-score
/// filesets. The first fileset's SNP column is kept; subsequent filesets must
/// have identical SNP columns. Each LD column is renamed `col_<i>`.
pub fn read_ldscore_fromlist(flist: &[String]) -> Result<LdScores> {
    if flist.is_empty() {
        return Err(LdscError::InvalidInput(
            "ldscore_fromlist: empty list".into(),
        ));
    }
    let mut combined: Option<LdScores> = None;
    for (i, fh) in flist.iter().enumerate() {
        let y = read_ldscore(fh, None)?;
        if i > 0 {
            let base = combined.as_ref().unwrap();
            if !series_eq(&base.snp, &y.snp) {
                return Err(LdscError::Parse {
                    context: "ldscore_fromlist".into(),
                    reason: "LD Scores for concatenation must have identical SNP columns".into(),
                });
            }
        }
        let base = combined.get_or_insert_with(|| LdScores {
            snp: y.snp.clone(),
            colnames: Vec::new(),
            cols: Vec::new(),
        });
        for (name, col) in y.colnames.iter().zip(y.cols.iter()) {
            base.colnames.push(format!("{name}_{i}"));
            base.cols.push(col.clone());
        }
    }
    Ok(combined.expect("non-empty list handled above"))
}

/// Resolve the concrete `.l2.ldscore` file paths for a prefix (single file, or
/// one per present chromosome). Mirrors `ldscore(fh, num)`.
fn resolve_ldscore_files(prefix: &str, num: Option<u32>) -> Result<Vec<(u32, String)>> {
    let suffix = ".l2.ldscore";
    match num {
        None => {
            let (sfx, _comp) = which_compression(&format!("{prefix}{suffix}"))?;
            Ok(vec![(0, format!("{prefix}{suffix}{sfx}"))])
        }
        Some(n) => {
            let chrs = get_present_chrs(prefix, n + 1);
            if chrs.is_empty() {
                return Err(LdscError::Parse {
                    context: "ldscore".into(),
                    reason: format!("no chromosome files found for prefix {prefix}"),
                });
            }
            let first = sub_chr(prefix, chrs[0]);
            let (sfx, _comp) = which_compression(&format!("{first}{suffix}"))?;
            Ok(chrs
                .into_iter()
                .map(|c| (c, format!("{}{suffix}{sfx}", sub_chr(prefix, c))))
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// M files  (parse.M, M_fromlist)
// ---------------------------------------------------------------------------

/// `M(fh, num, N, common)` — parse `.lN.M[_5_50]` files. Reads the first line
/// of each file, splits on whitespace, parses each token to float. Across
/// chromosomes, values are summed elementwise. Errors on any non-numeric token.
pub fn read_m(prefix: &str, num: Option<u32>, n: u32, common: bool) -> Result<Vec<f64>> {
    let suffix = if common {
        format!(".l{n}.M_5_50")
    } else {
        format!(".l{n}.M")
    };
    let paths: Vec<String> = match num {
        None => vec![format!("{prefix}{suffix}")],
        Some(nn) => {
            let chrs = get_present_chrs(prefix, nn + 1);
            chrs.into_iter()
                .map(|c| format!("{}{suffix}", sub_chr(prefix, c)))
                .collect()
        }
    };
    if paths.is_empty() {
        return Err(LdscError::Parse {
            context: "M".into(),
            reason: format!("no chromosome M files found for prefix {prefix}"),
        });
    }
    let mut acc: Option<Vec<f64>> = None;
    for path in &paths {
        let lines = read_lines(path)?;
        let first = lines.into_iter().next().ok_or_else(|| LdscError::Parse {
            context: "M".into(),
            reason: format!("{path} is empty"),
        })?;
        let vals: Vec<f64> = first
            .split_whitespace()
            .map(parse_f64_strict)
            .collect::<Result<_>>()?;
        match &mut acc {
            None => acc = Some(vals),
            Some(a) => {
                if a.len() != vals.len() {
                    return Err(LdscError::DimensionMismatch(format!(
                        "M: chromosome file {path} has {} values, expected {}",
                        vals.len(),
                        a.len()
                    )));
                }
                for (x, v) in a.iter_mut().zip(vals.iter()) {
                    *x += v;
                }
            }
        }
    }
    Ok(acc.expect("at least one path"))
}

/// `M_fromlist(flist, num, N, common)` — sideways concatenation of M files.
pub fn read_m_fromlist(
    flist: &[String],
    num: Option<u32>,
    n: u32,
    common: bool,
) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    for fh in flist {
        out.extend(read_m(fh, num, n, common)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Frequency files  (parse.frq_parser)
// ---------------------------------------------------------------------------

/// Per-SNP allele frequency. `frq_parser`: if a `MAF` column is present it is
/// renamed to `FRQ`.
#[derive(Debug, Clone)]
pub struct Frq {
    pub snp: Vec<String>,
    pub frq: Vec<f64>,
}

pub fn read_frq(path: &str) -> Result<Frq> {
    let table = read_table(path)?;
    let frq_name = if table.col_idx("MAF").is_ok() {
        "MAF"
    } else {
        "FRQ"
    };
    let snp = table.column("SNP")?;
    let frq: Vec<f64> = table
        .column(frq_name)?
        .iter()
        .map(|s| parse_f64_cell(s))
        .collect();
    Ok(Frq { snp, frq })
}

// ---------------------------------------------------------------------------
// CTS files  (parse.read_cts)
// ---------------------------------------------------------------------------

/// `read_cts(fh, match_snps)` — two-column headerless file (SNP, value). The
/// SNP column must be identical to `match_snps`; returns the values.
pub fn read_cts(path: &str, match_snps: &[String]) -> Result<Vec<f64>> {
    let lines = read_lines(path)?;
    let rows: Vec<Vec<String>> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(str::to_owned).collect())
        .collect();
    let snp: Vec<String> = rows
        .iter()
        .map(|r| r.first().cloned().unwrap_or_default())
        .collect();
    if !series_eq(&snp, match_snps) {
        return Err(LdscError::Parse {
            context: "cts".into(),
            reason: "--cts-bin and the .bim file must have identical SNP columns".into(),
        });
    }
    Ok(rows
        .iter()
        .map(|r| parse_f64_cell(r.get(1).map(|s| s.as_str()).unwrap_or(".")))
        .collect())
}

// ---------------------------------------------------------------------------
// Summary statistics  (parse.sumstats)
// ---------------------------------------------------------------------------

/// Parsed `.sumstats`: SNP, Z, N, and (optionally) A1/A2 columns. Numeric
/// missing values (`.`) become `NaN`.
#[derive(Debug, Clone, Default)]
pub struct SumStats {
    pub snp: Vec<String>,
    pub z: Vec<f64>,
    pub n: Vec<f64>,
    pub a1: Option<Vec<String>>,
    pub a2: Option<Vec<String>>,
}

impl SumStats {
    pub fn len(&self) -> usize {
        self.snp.len()
    }
    pub fn is_empty(&self) -> bool {
        self.snp.is_empty()
    }
}

/// `sumstats(fh, alleles, dropna)` — parse SNP/Z/N (and A1/A2 when `alleles`).
/// Missing values follow pandas `na_values='.'`. When `dropna`, rows with any
/// missing value are dropped.
pub fn read_sumstats(path: &str, alleles: bool, dropna: bool) -> Result<SumStats> {
    let table = read_table(path)?;
    // Required columns. Missing → pandas raises ValueError (usecols mismatch).
    let snp_i = table.col_idx("SNP")?;
    let z_i = table.col_idx("Z")?;
    let n_i = table.col_idx("N")?;
    let (a1_i, a2_i) = if alleles {
        (Some(table.col_idx("A1")?), Some(table.col_idx("A2")?))
    } else {
        (None, None)
    };

    let mut snp = Vec::new();
    let mut z = Vec::new();
    let mut n = Vec::new();
    let mut a1 = a1_i.map(|_| Vec::new());
    let mut a2 = a2_i.map(|_| Vec::new());

    for row in &table.rows {
        let s_snp = row.get(snp_i).cloned().unwrap_or_default();
        let s_z = parse_f64_cell(row.get(z_i).map(|s| s.as_str()).unwrap_or("."));
        let s_n = parse_f64_cell(row.get(n_i).map(|s| s.as_str()).unwrap_or("."));
        let s_a1 = a1_i.map(|j| row.get(j).cloned().unwrap_or_default());
        let s_a2 = a2_i.map(|j| row.get(j).cloned().unwrap_or_default());

        if dropna {
            let any_missing = is_na(&s_snp)
                || s_z.is_nan()
                || s_n.is_nan()
                || s_a1.as_deref().is_some_and(is_na)
                || s_a2.as_deref().is_some_and(is_na);
            if any_missing {
                continue;
            }
        }
        snp.push(s_snp);
        z.push(s_z);
        n.push(s_n);
        if let Some(v) = a1.as_mut() {
            v.push(s_a1.unwrap_or_default());
        }
        if let Some(v) = a2.as_mut() {
            v.push(s_a2.unwrap_or_default());
        }
    }

    Ok(SumStats { snp, z, n, a1, a2 })
}

// ---------------------------------------------------------------------------
// Annotation files  (parse.annot, annot_parser)
// ---------------------------------------------------------------------------

/// Overlap matrix `Aᵀ A` and total SNP count `M_tot` from `.annot` files.
/// Port of `parse.annot(fh_list, num, frqfile)`.
#[derive(Debug, Clone)]
pub struct AnnotOverlap {
    /// `n_annot × n_annot` overlap matrix (row-major).
    pub matrix: Vec<Vec<f64>>,
    pub m_tot: usize,
    pub n_annot: usize,
}

/// Parse a single `.annot` file into its numeric annotation matrix (dropping
/// the SNP/CHR/BP/CM columns). `annot_parser`.
pub fn annot_matrix(path: &str) -> Result<Vec<Vec<f64>>> {
    let table = read_table(path)?;
    let drop = ["SNP", "CHR", "BP", "CM"];
    let keep_idx: Vec<usize> = (0..table.header.len())
        .filter(|&j| !drop.contains(&table.header[j].as_str()))
        .collect();
    let mut out = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        out.push(
            keep_idx
                .iter()
                .map(|&j| parse_f64_cell(row.get(j).map(|s| s.as_str()).unwrap_or(".")))
                .collect(),
        );
    }
    Ok(out)
}

/// `parse.annot` — for each fileset, read the annotation matrix (optionally
/// filtered to common variants by a `.frq` file's 5–50 MAF band), concatenate
/// sideways, and compute `Aᵀ A` summed across chromosomes.
pub fn read_annot(
    fh_list: &[String],
    num: Option<u32>,
    frqfile: Option<&str>,
) -> Result<AnnotOverlap> {
    if frqfile.is_some() {
        // The 5–50 MAF filtering of the annot matrix by a `.frq` file is wired
        // up in the overlap-output driver (Phase 4); the raw overlap matrix is
        // computed over all rows here.
    }
    let annot_suffix = ".annot";
    let targets: Vec<(u32, Vec<String>)> = match num {
        None => {
            let mut paths = Vec::new();
            for fh in fh_list {
                let (sfx, _c) = which_compression(&format!("{fh}{annot_suffix}"))?;
                paths.push(format!("{fh}{annot_suffix}{sfx}"));
            }
            vec![(0, paths)]
        }
        Some(n) => {
            let chrs = get_present_chrs(&fh_list[0], n + 1);
            chrs.into_iter()
                .map(|c| {
                    let mut paths = Vec::new();
                    for fh in fh_list {
                        let sub = sub_chr(fh, c);
                        let (sfx, _c2) =
                            which_compression(&format!("{sub}{annot_suffix}"))?;
                        paths.push(format!("{sub}{annot_suffix}{sfx}"));
                    }
                    Ok((c, paths))
                })
                .collect::<Result<Vec<_>>>()?
        }
    };

    let mut m_tot = 0usize;
    let mut n_annot = 0usize;
    let mut sum_ata: Vec<f64> = Vec::new();

    for (_chr, paths) in &targets {
        let mats: Vec<Vec<Vec<f64>>> = {
            let mut out = Vec::new();
            for path in paths {
                out.push(annot_matrix(path)?);
            }
            out
        };
        // sideways concat
        let n_snps = mats[0].len();
        let total_annot: usize = mats
            .iter()
            .map(|m| m.first().map(|r| r.len()).unwrap_or(0))
            .sum();
        let mut concat: Vec<Vec<f64>> = Vec::with_capacity(n_snps);
        for r in 0..n_snps {
            let mut row = Vec::with_capacity(total_annot);
            for m in &mats {
                row.extend_from_slice(&m[r]);
            }
            concat.push(row);
        }
        n_annot = total_annot;
        let ata = matmul_ata(&concat, n_annot);
        if sum_ata.is_empty() {
            sum_ata = ata;
        } else {
            for cell in ata.into_iter().enumerate() {
                sum_ata[cell.0] += cell.1;
            }
        }
        m_tot += n_snps;
    }

    let matrix = (0..n_annot)
        .map(|a| (0..n_annot).map(|b| sum_ata[a * n_annot + b]).collect())
        .collect();
    Ok(AnnotOverlap {
        matrix,
        m_tot,
        n_annot,
    })
}

fn matmul_ata(a: &[Vec<f64>], n_annot: usize) -> Vec<f64> {
    let mut out = vec![0.0; n_annot * n_annot];
    for row in a {
        for x in 0..n_annot {
            for y in 0..n_annot {
                out[x * n_annot + y] += row[x] * row[y];
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// PLINK .bim / .fam + filter / annot ID lists  (parse.__ID_List_Factory__)
// ---------------------------------------------------------------------------

/// A one-column ID list with `loj` (left-outer-join) index lookup. Port of the
/// `IDContainer` produced by `__ID_List_Factory__`.
#[derive(Debug, Clone)]
pub struct IdList {
    pub ids: Vec<String>,
}

impl IdList {
    pub fn n(&self) -> usize {
        self.ids.len()
    }

    /// `loj(externalDf)` — indices of `self.ids` entries that appear in
    /// `external` (the filter file's first column).
    pub fn loj(&self, external: &[String]) -> Vec<usize> {
        let set: HashSet<&str> = external.iter().map(|s| s.as_str()).collect();
        self.ids
            .iter()
            .enumerate()
            .filter(|(_, id)| set.contains(id.as_str()))
            .map(|(i, _)| i)
            .collect()
    }
}

/// PLINK `.bim`: CHR SNP CM BP A1 A2. Filename must end in `.bim`.
#[derive(Debug, Clone)]
pub struct BimFile {
    pub chr: Vec<i64>,
    pub snp: Vec<String>,
    pub cm: Vec<f64>,
    pub bp: Vec<i64>,
    pub a1: Vec<String>,
    pub a2: Vec<String>,
}

impl BimFile {
    /// `PlinkBIMFile(fname)` — 6 whitespace-delimited columns, no header.
    pub fn read(path: &str) -> Result<BimFile> {
        require_ext(path, ".bim")?;
        let lines = read_lines(path)?;
        let mut chr = Vec::new();
        let mut snp = Vec::new();
        let mut cm = Vec::new();
        let mut bp = Vec::new();
        let mut a1 = Vec::new();
        let mut a2 = Vec::new();
        for line in lines.iter().filter(|l| !l.trim().is_empty()) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 6 {
                return Err(LdscError::Parse {
                    context: "bim".into(),
                    reason: format!("expected 6 columns, got {}", f.len()),
                });
            }
            chr.push(f[0].parse::<i64>().unwrap_or(0));
            snp.push(f[1].to_owned());
            cm.push(parse_f64_cell(f[2]));
            bp.push(f[3].parse::<i64>().unwrap_or(0));
            a1.push(f[4].to_owned());
            a2.push(f[5].to_owned());
        }
        Ok(BimFile {
            chr,
            snp,
            cm,
            bp,
            a1,
            a2,
        })
    }

    pub fn n(&self) -> usize {
        self.snp.len()
    }
    pub fn id_list(&self) -> IdList {
        IdList {
            ids: self.snp.clone(),
        }
    }
}

/// PLINK `.fam`: IID is the second column. Filename must end in `.fam`.
#[derive(Debug, Clone)]
pub struct FamFile {
    pub iid: Vec<String>,
}

impl FamFile {
    pub fn read(path: &str) -> Result<FamFile> {
        require_ext(path, ".fam")?;
        let lines = read_lines(path)?;
        let iid = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                f.get(1).copied().unwrap_or("").to_owned()
            })
            .collect();
        Ok(FamFile { iid })
    }
    pub fn n(&self) -> usize {
        self.iid.len()
    }
    pub fn id_list(&self) -> IdList {
        IdList {
            ids: self.iid.clone(),
        }
    }
}

/// `FilterFile(fname)` — one-column ID list, no header, no extension required.
pub fn read_filter_file(path: &str) -> Result<IdList> {
    let lines = read_lines(path)?;
    let ids = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().next().unwrap_or("").to_owned())
        .collect();
    Ok(IdList { ids })
}

/// `AnnotFile(fname)` — header row; SNP is column index 2. Keeps the file
/// header and all rows.
#[derive(Debug, Clone)]
pub struct AnnotFile {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl AnnotFile {
    /// `AnnotFile(fname)`: header=0, all columns; SNP = column 2.
    pub fn read(path: &str) -> Result<AnnotFile> {
        let t = read_table(path)?;
        Ok(AnnotFile {
            header: t.header,
            rows: t.rows,
        })
    }
    pub fn n(&self) -> usize {
        self.rows.len()
    }
    pub fn id_list(&self) -> Result<IdList> {
        if self.header.len() <= 2 || self.header[2] != "SNP" {
            return Err(LdscError::Parse {
                context: "annot".into(),
                reason: "column 2 is not 'SNP'".into(),
            });
        }
        let ids = self
            .rows
            .iter()
            .map(|r| r.get(2).cloned().unwrap_or_default())
            .collect();
        Ok(IdList { ids })
    }
}

/// `ThinAnnotFile(fname)` — header row; annotations only (no SNP/geo columns).
pub fn read_thin_annot_file(path: &str) -> Result<Table> {
    read_table(path)
}

fn require_ext(path: &str, ext: &str) -> Result<()> {
    if !path.ends_with(ext) {
        return Err(LdscError::Plink(format!(
            "{path} filename must end in {ext}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(p: &str) -> String {
        format!("tests/data/{p}")
    }

    #[test]
    fn series_eq_basic() {
        assert!(series_eq(
            &["a".into(), "b".into(), "c".into()],
            &["a".into(), "b".into(), "c".into()]
        ));
        assert!(!series_eq(&["a".into()], &["a".into(), "b".into()]));
        assert!(!series_eq(
            &["a".into(), "b".into()],
            &["a".into(), "c".into()]
        ));
    }

    #[test]
    fn get_compression_suffix() {
        assert_eq!(get_compression("foo.gz"), Compression::Gzip);
        assert_eq!(get_compression("foo.bz2"), Compression::Bzip2);
        assert_eq!(get_compression("foo.bar"), Compression::None);
    }

    #[test]
    fn which_compression_finds_gz() {
        let (sfx, c) = which_compression(&data("parse_test/test.l2.ldscore")).unwrap();
        assert_eq!(sfx, ".gz");
        assert_eq!(c, Compression::Gzip);
    }

    #[test]
    fn read_cts_matches_snp() {
        let m = ["rs1".into(), "rs2".into(), "rs3".into()];
        let v = read_cts(&data("parse_test/test.cts"), &m).unwrap();
        assert_eq!(v, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn read_cts_rejects_mismatch() {
        let m = ["rs1".into(), "rs2".into()];
        assert!(read_cts(&data("parse_test/test.cts"), &m).is_err());
    }

    #[test]
    fn sumstats_dropna() {
        // test.sumstats has 2 rows; the second has SNP '.' → dropped.
        let s = read_sumstats(&data("parse_test/test.sumstats"), true, true).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s.snp, vec!["rs1".to_string()]);
        assert!(s.a1.is_some() && s.a2.is_some());
    }

    #[test]
    fn sumstats_missing_z_errors() {
        // A .ldscore.gz has no Z column → error (pandas usecols mismatch).
        assert!(read_sumstats(&data("parse_test/test.l2.ldscore.gz"), false, true).is_err());
    }

    #[test]
    fn frq_parser_with_frq_col() {
        let x = read_frq(&data("parse_test/test1.frq")).unwrap();
        assert_eq!(x.snp, (0..8).map(|i| format!("rs_{i}")).collect::<Vec<_>>());
        assert_eq!(x.frq, vec![0.01, 0.1, 0.7, 0.2, 0.2, 0.2, 0.99, 0.03]);
    }

    #[test]
    fn frq_parser_gz_maf_renamed() {
        // test2.frq.gz has MAF column → treated as FRQ.
        let x = read_frq(&data("parse_test/test2.frq.gz")).unwrap();
        assert_eq!(x.frq, vec![0.01, 0.1, 0.3, 0.2, 0.2, 0.2, 0.01, 0.03]);
    }

    #[test]
    fn ldscore_single_file() {
        let x = read_ldscore(&data("parse_test/test"), None).unwrap();
        assert_eq!(
            x.snp,
            (1..=22).map(|i| format!("rs{i}")).collect::<Vec<_>>()
        );
        assert_eq!(x.colnames, vec!["AL2".to_string(), "BL2".into()]);
        assert_eq!(x.cols[0], (1..=22).map(|i| i as f64).collect::<Vec<_>>());
        assert_eq!(
            x.cols[1],
            (1..=22).map(|i| 2.0 * i as f64).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ldscore_fromlist_concat() {
        let fh = data("parse_test/test");
        let x = read_ldscore_fromlist(&[fh.clone(), fh]).unwrap();
        assert_eq!(x.snp.len(), 22);
        assert_eq!(x.colnames.len(), 4); // AL2_0 BL2_0 AL2_1 BL2_1
        assert_eq!(x.cols.len(), 4);
        // cols 0,1 == cols 2,3
        assert_eq!(x.cols[0], x.cols[2]);
        assert_eq!(x.cols[1], x.cols[3]);
    }

    #[test]
    fn ldscore_fromlist_mismatch_errors() {
        let a = data("parse_test/test");
        let b = data("parse_test/test2"); // different SNPs
        assert!(read_ldscore_fromlist(&[a, b]).is_err());
    }

    #[test]
    fn m_single_file() {
        let x = read_m(&data("parse_test/test"), None, 2, false).unwrap();
        assert_eq!(x, vec![1000.0, 2000.0, 3000.0]);
    }

    #[test]
    fn m_bad_errors() {
        // "Nan 100 ." — the "." is non-numeric → error.
        assert!(read_m(&data("parse_test/test_bad"), None, 2, false).is_err());
    }

    #[test]
    fn bim_file() {
        let b = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        assert_eq!(b.n(), 8);
        assert_eq!(b.snp, (0..8).map(|i| format!("rs_{i}")).collect::<Vec<_>>());
    }

    #[test]
    fn bim_bad_filename() {
        assert!(BimFile::read(&data("plink_test/plink.fam")).is_err());
    }

    #[test]
    fn fam_file() {
        let f = FamFile::read(&data("plink_test/plink.fam")).unwrap();
        assert_eq!(f.n(), 5);
        assert_eq!(f.iid, (0..5).map(|i| format!("per{i}")).collect::<Vec<_>>());
    }

    #[test]
    fn fam_bad_filename() {
        assert!(FamFile::read(&data("plink_test/plink.bim")).is_err());
    }

    #[test]
    fn loj_keeps_intersect() {
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        let ids = bim.id_list();
        let keep = vec!["rs_1".into(), "rs_4".into()];
        let idx = ids.loj(&keep);
        assert_eq!(idx, vec![1, 4]);
    }
}
