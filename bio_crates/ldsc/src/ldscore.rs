//! LD-score computation from PLINK genotypes — a faithful port of the
//! windowed-correlation engine in `ldscore/ldscore.py` (`getBlockLefts`,
//! `block_left_to_right`, and `__GenotypeArrayInMemory__.__corSumVarBlocks__`).
//!
//! Given standardized genotype columns (from [`crate::bedio::PlinkBed::next_snps`])
//! and a window over SNP coordinates, the LD score of SNP `j` for annotation
//! `a` is
//!
//! ```text
//! L(j,a) = Σ_{p ∈ window(j)} annot[p][a] · func(r_{j,p}),
//! ```
//!
//! where `r_{j,p}` is the Pearson correlation between SNPs `j` and `p`, and
//! `func` is the L2-unbiased correction `r² − (1−r²)/(n−2)` for the default
//! `--l2` LD score.

use crate::bedio::PlinkBed;
use crate::{LdscError, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::Write;

/// Which function of the correlation `r` to accumulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LdFunc {
    /// `r² − (1−r²)/(n−2)` — the default `--l2` (`ldScoreVarBlocks`) LD score.
    L2Unbiased,
    /// `r²` — biased L2 (`ldScoreBlockJackknife`).
    L2Biased,
    /// `r` — L1.
    L1,
}

impl LdFunc {
    fn apply(self, r: f64, n: usize) -> f64 {
        match self {
            LdFunc::L2Unbiased => PlinkBed::l2_unbiased(r * r, n),
            LdFunc::L2Biased => r * r,
            LdFunc::L1 => r,
        }
    }
}

/// `getBlockLefts(coords, max_dist)` — two-pointer (coords must be sorted).
/// `block_left[j] = min{ k : |coords[k] − coords[j]| ≤ max_dist }`.
pub fn get_block_lefts(coords: &[f64], max_dist: f64) -> Vec<usize> {
    let m = coords.len();
    let mut block_left = vec![0usize; m];
    let mut j = 0usize;
    for i in 0..m {
        while j < m && (coords[j] - coords[i]).abs() > max_dist {
            j += 1;
        }
        block_left[i] = j;
    }
    block_left
}

/// `block_left_to_right(block_left)` — `block_right[i] = max{ k : block_left[k] ≤ i }`.
pub fn block_left_to_right(block_left: &[usize]) -> Vec<usize> {
    let m = block_left.len();
    let mut block_right = vec![0usize; m];
    let mut j = 0usize;
    for i in 0..m {
        while j < m && block_left[j] <= i {
            j += 1;
        }
        block_right[i] = j;
    }
    block_right
}

/// Compute `Aᵀ·(B/n)` then apply `func` elementwise. `A` is `n×b`, `B` is `n×c`;
/// returns `b×c`. Port of `np.dot(A.T, B/n)` followed by `func(...)`.
fn at_b_func(
    a: &[Vec<f64>],
    b: &[Vec<f64>],
    n: usize,
    func: LdFunc,
    n_samples: usize,
) -> Vec<Vec<f64>> {
    let bb = if a.is_empty() { 0 } else { a[0].len() }; // b cols
    let cc = if b.is_empty() { 0 } else { b[0].len() }; // c cols
    let mut out = vec![vec![0.0; cc]; bb];
    for i in 0..bb {
        for k in 0..cc {
            let mut s = 0.0;
            for row in 0..n {
                s += a[row][i] * b[row][k];
            }
            out[i][k] = func.apply(s / n as f64, n_samples);
        }
    }
    out
}

/// `__corSumVarBlocks__` — the core windowed LD-score accumulation. Returns an
/// `m × n_a` matrix where row `j` is the per-annotation LD score of SNP `j`.
///
/// * `bed` — genotype array (its `next_snps` cursor is consumed sequentially,
///   exactly like Python's `snp_getter`).
/// * `block_left` — from [`get_block_lefts`].
/// * `c` — chunk size (LDSC `--chunk-size`, default 50).
/// * `annot` — `m × n_a` annotation matrix, or `None` for a single all-ones
///   annotation.
/// * `func` — which function of `r` to accumulate.
pub fn cor_sum_var_blocks(
    bed: &mut PlinkBed,
    block_left: &[usize],
    mut c: usize,
    annot: Option<&[Vec<f64>]>,
    func: LdFunc,
) -> Result<Vec<Vec<f64>>> {
    let (m, n) = (bed.m, bed.n);
    if block_left.len() != m {
        return Err(LdscError::DimensionMismatch(format!(
            "cor_sum_var_blocks: block_left len {} != m {m}",
            block_left.len()
        )));
    }
    // annot (default ones), shape m × n_a
    let (annot_owned, annot_ref): (Vec<Vec<f64>>, &[Vec<f64>]) = match annot {
        Some(a) => {
            if a.len() != m {
                return Err(LdscError::DimensionMismatch(
                    "Incorrect number of SNPs in annot".into(),
                ));
            }
            (Vec::new(), a)
        }
        None => (vec![vec![1.0]; m], &[][..]),
    };
    let annot: &[Vec<f64>] = if annot_ref.is_empty() {
        &annot_owned
    } else {
        annot_ref
    };
    let n_a = annot.first().map(|r| r.len()).unwrap_or(1);

    let mut cor_sum = vec![vec![0.0; n_a]; m];

    // block_sizes = ceil((arange(m) - block_left)/c)*c
    let block_sizes: Vec<usize> = (0..m)
        .map(|i| {
            let raw = (i as f64) - (block_left[i] as f64);
            (raw / c as f64).ceil() as usize * c
        })
        .collect();

    // b = first index with block_left>0 (else m), rounded up to multiple of c
    let first_gt = block_left.iter().position(|&x| x > 0).unwrap_or(m);
    let mut b = ((first_gt as f64) / c as f64).ceil() as usize * c;
    if b > m {
        c = 1;
        b = m;
    }

    let mut l_a = 0usize;
    let mut a_mat: Vec<Vec<f64>> = bed.next_snps(b, false)?; // n × b
    // within-block
    for l_b in (0..b).step_by(c) {
        // B = A[:, l_b:l_b+c]
        let bmat: Vec<Vec<f64>> = a_mat
            .iter()
            .map(|row| row[l_b..(l_b + c).min(b)].to_vec())
            .collect();
        let cc = bmat.first().map(|r| r.len()).unwrap_or(0);
        let rfunc = at_b_func(&a_mat, &bmat, n, func, n); // b × cc
        // cor_sum[l_a:l_a+b] += rfunc · annot[l_b:l_b+cc]
        for i in 0..b {
            for k in 0..cc {
                let ak = &annot[l_b + k];
                for col in 0..n_a {
                    cor_sum[l_a + i][col] += rfunc[i][k] * ak[col];
                }
            }
        }
    }

    // right of block
    let b0 = b;
    let md = c * (m / c);
    let end = if md != m { md + 1 } else { md };
    let mut bmat_prev_cols = 0usize;
    let mut last_b: Vec<Vec<f64>> = Vec::new();
    for l_b in (b0..end).step_by(c) {
        let old_b = b;
        b = block_sizes[l_b];
        if l_b > b0 && b > 0 {
            // A = hstack(A[:, old_b-b+c:old_b], B_prev)
            let take_from = old_b + c - b; // old_b - b + c
            let mut new_a: Vec<Vec<f64>> = Vec::with_capacity(n);
            let bprev = &a_mat; // current A (n × old_b) ; but we need B_prev too
            // Reconstruct: keep A[:, take_from:old_b] then append B_prev (last c cols as B_prev)
            // B_prev is the previous chunk B (n × c). We carry it in `last_b`.
            for row in 0..n {
                let mut rrow: Vec<f64> = bprev[row][take_from..old_b].to_vec();
                rrow.extend_from_slice(&last_b[row][0..c.min(last_b[row].len())]);
                new_a.push(rrow);
            }
            a_mat = new_a;
            l_a += old_b - b + c;
        } else if l_b == b0 && b > 0 {
            let from = b0 - b;
            a_mat = a_mat.iter().map(|row| row[from..b0].to_vec()).collect();
            l_a = b0 - b;
        } else if b == 0 {
            a_mat = vec![Vec::new(); n];
            l_a = l_b;
        }
        let mut cc = c;
        if l_b == md {
            cc = m - md;
        }
        if b != old_b {
            // rfuncAB reallocated implicitly
        }
        let bmat = bed.next_snps(cc, false)?; // n × cc
        bmat_prev_cols = cc;
        // p1 = all annot[l_a:l_a+b]==0, p2 = all annot[l_b:l_b+cc]==0
        let p1 = (l_a..l_a + b).all(|i| annot[i].iter().all(|&x| x == 0.0));
        let p2 = (l_b..l_b + cc).all(|i| annot[i].iter().all(|&x| x == 0.0));
        if p1 && p2 {
            last_b = bmat;
            continue;
        }
        let rfunc = at_b_func(&a_mat, &bmat, n, func, n); // b × cc
        // cor_sum[l_a:l_a+b] += rfunc · annot[l_b:l_b+cc]
        for i in 0..b {
            for k in 0..cc {
                let ak = &annot[l_b + k];
                for col in 0..n_a {
                    cor_sum[l_a + i][col] += rfunc[i][k] * ak[col];
                }
            }
        }
        // cor_sum[l_b:l_b+cc] += annot[l_a:l_a+b].T · rfunc   → (cc × n_a)
        for k in 0..cc {
            for i in 0..b {
                let ai = &annot[l_a + i];
                for col in 0..n_a {
                    cor_sum[l_b + k][col] += ai[col] * rfunc[i][k];
                }
            }
        }
        // rfuncBB = func(B.T · B/n)  (cc × cc)
        for k in 0..cc {
            for kk in 0..cc {
                let mut s = 0.0;
                for row in 0..n {
                    s += bmat[row][k] * bmat[row][kk];
                }
                let rf = func.apply(s / n as f64, n);
                let akk = &annot[l_b + kk];
                for col in 0..n_a {
                    cor_sum[l_b + k][col] += rf * akk[col];
                }
            }
        }
        last_b = bmat;
    }
    let _ = bmat_prev_cols;

    Ok(cor_sum)
}

// carry the previous chunk B between right-of-block iterations
// (declared as a local via a small helper thread; implemented inline above with `last_b`)

/// Write LD-score output files for the `--l2` path: `<prefix>.l2.ldscore.gz`
/// (columns `CHR SNP BP <ld_colnames>`, `%.3f`), `<prefix>.l2.M`, and
/// `<prefix>.l2.M_5_50`. Port of `ldsc.py ldscore()` file output.
pub fn write_ldscore(
    prefix: &str,
    bed: &PlinkBed,
    ld_score: &[Vec<f64>],
    ld_colnames: &[String],
    m_annot: &[f64],
    m_5_50: &[f64],
) -> Result<()> {
    let ldscore_path = format!("{prefix}.l2.ldscore.gz");
    let f = File::create(&ldscore_path)?;
    let mut enc = GzEncoder::new(f, Compression::default());
    // header: CHR SNP BP <ld cols>  (CM/MAF dropped, matching ldsc.py)
    writeln!(enc, "CHR\tSNP\tBP\t{}", ld_colnames.join("\t"))?;
    for j in 0..bed.m {
        write!(enc, "{}\t{}\t{}", bed.chr[j], bed.snp[j], bed.bp[j])?;
        for a in 0..ld_colnames.len() {
            write!(enc, "\t{:.3}", ld_score[j][a])?;
        }
        writeln!(enc)?;
    }
    enc.finish()?;

    let mut mfile = File::create(format!("{prefix}.l2.M"))?;
    writeln!(
        mfile,
        "{}",
        m_annot
            .iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join("\t")
    )?;
    let mut m550 = File::create(format!("{prefix}.l2.M_5_50"))?;
    writeln!(
        m550,
        "{}",
        m_5_50
            .iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join("\t")
    )?;
    Ok(())
}

/// High-level: compute the default (L2-unbiased) LD scores for a genotype array
/// and a window. Returns `m × n_a` LD scores and the per-annotation `M` and
/// `M_5_50` (SNP-count sums of `annot`, total and common-MAF).
pub struct LdScoreOutput {
    /// `m × n_a` LD scores.
    pub ld_score: Vec<Vec<f64>>,
    /// per-annotation `Σ annot` (the `.l2.M` values).
    pub m_annot: Vec<f64>,
    /// per-annotation `Σ annot` restricted to SNPs with MAF > 0.05.
    pub m_5_50: Vec<f64>,
}

/// `ldScoreVarBlocks(block_left, c, annot)` — default L2-unbiased LD score.
pub fn ld_score_var_blocks(
    bed: &mut PlinkBed,
    block_left: &[usize],
    c: usize,
    annot: Option<&[Vec<f64>]>,
) -> Result<LdScoreOutput> {
    let n_a = annot.and_then(|a| a.first()).map(|r| r.len()).unwrap_or(1);
    let ld_score = cor_sum_var_blocks(bed, block_left, c, annot, LdFunc::L2Unbiased)?;
    let m = bed.m;
    // M = Σ annot over SNPs; M_5_50 = Σ annot over SNPs with maf>0.05
    let mut m_annot = vec![0.0; n_a];
    let mut m_5_50 = vec![0.0; n_a];
    for j in 0..m {
        let common = bed.maf[j] > 0.05;
        for a in 0..n_a {
            let v = annot.map(|an| an[j][a]).unwrap_or(1.0);
            m_annot[a] += v;
            if common {
                m_5_50[a] += v;
            }
        }
    }
    Ok(LdScoreOutput {
        ld_score,
        m_annot,
        m_5_50,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bedio::PlinkBed;
    use crate::io::BimFile;

    fn data(p: &str) -> String {
        format!("tests/data/{p}")
    }

    #[test]
    fn get_block_lefts_matches_python() {
        // (coords, max_dist, expected)
        let cases: Vec<(&[f64], f64, &[usize])> = vec![
            (&[1.0, 2.0, 3.0, 4.0, 5.0], 5.0, &[0, 0, 0, 0, 0]),
            (&[1.0, 2.0, 3.0, 4.0, 5.0], 0.0, &[0, 1, 2, 3, 4]),
            (&[1.0, 4.0, 6.0, 7.0, 7.0, 8.0], 2.0, &[0, 1, 1, 2, 2, 2]),
        ];
        for (coords, md, exp) in cases {
            assert_eq!(
                get_block_lefts(coords, md),
                exp,
                "coords={coords:?} md={md}"
            );
        }
    }

    #[test]
    fn block_left_to_right_matches_python() {
        let cases: Vec<(&[usize], &[usize])> = vec![
            (&[0, 0, 0, 0, 0], &[5, 5, 5, 5, 5]),
            (&[0, 1, 2, 3, 4, 5], &[1, 2, 3, 4, 5, 6]),
            (&[0, 0, 2, 2], &[2, 2, 4, 4]),
        ];
        for (bl, exp) in cases {
            assert_eq!(block_left_to_right(bl), exp, "bl={bl:?}");
        }
    }

    /// Independent naive LD score: for each j, Σ_{p: |coord_j-coord_p|<=max_dist}
    /// annot[p]·func(r_jp). Standardized genotypes via next_snps.
    fn naive_ld(
        path: &str,
        bim: &BimFile,
        coords: &[f64],
        max_dist: f64,
        annot: Option<&[Vec<f64>]>,
        n_indiv: usize,
    ) -> (Vec<Vec<f64>>, Vec<usize>) {
        let mut bed = PlinkBed::read(path, n_indiv, bim, None, None, 0.0).unwrap();
        let m = bed.m;
        let n = bed.n;
        let g_all = bed.next_snps(m, false).unwrap(); // n × m
        let n_a = annot.and_then(|a| a.first()).map(|r| r.len()).unwrap_or(1);
        let mut out = vec![vec![0.0; n_a]; m];
        let block_left = get_block_lefts(coords, max_dist);
        // symmetric window: p in window(j) iff block_left[max(j,p)] <= min(j,p)
        for j in 0..m {
            for p in 0..m {
                let lo = j.min(p);
                let hi = j.max(p);
                if block_left[hi] <= lo {
                    // r_jp
                    let mut s = 0.0;
                    for row in 0..n {
                        s += g_all[row][j] * g_all[row][p];
                    }
                    let r = s / n as f64;
                    let f = LdFunc::L2Unbiased.apply(r, n);
                    for a in 0..n_a {
                        let av = annot.map(|an| an[p][a]).unwrap_or(1.0);
                        out[j][a] += f * av;
                    }
                }
            }
        }
        (out, block_left)
    }

    #[test]
    fn cor_sum_var_blocks_matches_naive_c1() {
        // plink.bed: 4 polymorphic SNPs, 5 indivs. ld_wind_snps=2 (coords 0..m).
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        let path = data("plink_test/plink.bed");
        let n = 5usize;
        let mut bed = PlinkBed::read(&path, n, &bim, None, None, 0.0).unwrap();
        let m = bed.m;
        let coords: Vec<f64> = (0..m).map(|i| i as f64).collect();
        let max_dist = 2.0;
        let (naive, block_left) = naive_ld(&path, &bim, &coords, max_dist, None, n);
        let got = cor_sum_var_blocks(&mut bed, &block_left, 1, None, LdFunc::L2Unbiased).unwrap();
        assert_eq!(got.len(), m);
        for j in 0..m {
            assert!(
                (got[j][0] - naive[j][0]).abs() < 1e-9,
                "SNP {j}: got {} naive {}",
                got[j][0],
                naive[j][0]
            );
        }
    }

    #[test]
    fn cor_sum_var_blocks_matches_naive_with_annot() {
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        let path = data("plink_test/plink.bed");
        let n = 5usize;
        let mut bed = PlinkBed::read(&path, n, &bim, None, None, 0.0).unwrap();
        let m = bed.m;
        let coords: Vec<f64> = (0..m).map(|i| i as f64).collect();
        let max_dist = 10.0; // whole-chromosome window (block_left all 0)
        // 2 annotations, arbitrary.
        let annot: Vec<Vec<f64>> = (0..m).map(|j| vec![(j as f64) % 2.0, 1.0]).collect();
        let (naive, block_left) = naive_ld(&path, &bim, &coords, max_dist, Some(&annot), n);
        let got =
            cor_sum_var_blocks(&mut bed, &block_left, 1, Some(&annot), LdFunc::L2Unbiased).unwrap();
        for j in 0..m {
            for a in 0..2 {
                assert!(
                    (got[j][a] - naive[j][a]).abs() < 1e-9,
                    "SNP {j} annot {a}: got {} naive {}",
                    got[j][a],
                    naive[j][a]
                );
            }
        }
    }

    #[test]
    fn write_then_read_ldscore_roundtrip() {
        use crate::io::read_ldscore;
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        let path = data("plink_test/plink.bed");
        let n = 5usize;
        let mut bed = PlinkBed::read(&path, n, &bim, None, None, 0.0).unwrap();
        let m = bed.m;
        let coords: Vec<f64> = (0..m).map(|i| i as f64).collect();
        let bl = get_block_lefts(&coords, 1.0);
        let out = ld_score_var_blocks(&mut bed, &bl, 1, None).unwrap();
        let tmp = std::env::temp_dir().join("ldsc_rt_test");
        write_ldscore(
            tmp.to_str().unwrap(),
            &bed,
            &out.ld_score,
            &["L2".to_string()],
            &out.m_annot,
            &out.m_5_50,
        )
        .unwrap();
        // read back
        let back = read_ldscore(tmp.to_str().unwrap(), None).unwrap();
        assert_eq!(back.colnames, vec!["L2".to_string()]);
        assert_eq!(back.snp, bed.snp);
        // values are written %.3f → tolerance 0.001
        for j in 0..m {
            assert!(
                (back.cols[0][j] - out.ld_score[j][0]).abs() < 0.002,
                "roundtrip mismatch at {j}"
            );
        }
        // M files
        let mtxt = std::fs::read_to_string(format!("{}.l2.M", tmp.to_str().unwrap())).unwrap();
        assert_eq!(mtxt.trim(), format!("{}", out.m_annot[0]));
    }
}
