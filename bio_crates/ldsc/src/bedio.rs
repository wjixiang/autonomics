//! PLINK `.bed` genotype reader — a faithful port of `PlinkBEDFile` and its
//! parent `__GenotypeArrayInMemory__` (`ldscore/ldscore.py:63-415`).
//!
//! Reads a SNP-major PLINK `.bed` file, optionally filters individuals and
//! monomorphic / low-MAF SNPs (the Plink bit-counting MAF filter), and exposes
//! [`PlinkBed::next_snps`] — mean-imputed, standardized genotype columns — which
//! the LD-score computation ([`crate::ldscore`]) consumes.
//!
//! ## `.bed` bit layout
//! Bytes `0..2` are the magic `0x6c 0x1b`; byte `2` is the mode (`0x01` =
//! SNP-major, the only mode LDSC accepts). Each SNP occupies
//! `nru = n + (4 - n%4)%4` bit-slots (2 bits per individual, padded to a whole
//! number of bytes). For individual `j` (0-indexed) the 2-bit raw code is
//! `(byte[j/4] >> (2*(j%4))) & 0b11`, decoded to a genotype as:
//!
//! | raw code | bitstring | genotype |
//! |----------|-----------|----------|
//! | `0b00`   | `00`      | 0 (hom)  |
//! | `0b10`   | `10`      | 9 (missing) |
//! | `0b01`   | `01`      | 1 (het)  |
//! | `0b11`   | `11`      | 2 (hom)  |
//!
//! (This matches LDSC's `bedcode = {2:'11', 9:'10', 1:'01', 0:'00'}` under a
//! little-endian bitarray.)

use crate::io::BimFile;
use crate::{LdscError, Result};

/// Missing-genotype sentinel (matches the `9` LDSC decodes from raw code `0b10`).
pub const MISSING: f64 = 9.0;

/// Decode a 2-bit raw code to a genotype value (0/1/2, or 9 = missing).
#[inline]
fn decode_code(code: u8) -> f64 {
    match code & 0b11 {
        0 => 0.0,
        2 => MISSING,
        1 => 1.0,
        3 => 2.0,
        _ => unreachable!(),
    }
}

/// `nru` — number of 2-bit slots per SNP (n padded up to a multiple of 4).
fn nru(n: usize) -> usize {
    let rem = n % 4;
    n + if rem != 0 { 4 - rem } else { 0 }
}

/// An in-memory PLINK `.bed` genotype array, post individual/MAF filtering.
/// Port of `PlinkBEDFile`.
pub struct PlinkBed {
    /// Number of SNPs retained (polymorphic, passing MAF).
    pub m: usize,
    /// Number of individuals retained.
    pub n: usize,
    /// Per-SNP metadata for retained SNPs: CHR, SNP, BP, CM (from the `.bim`).
    pub chr: Vec<i64>,
    pub snp: Vec<String>,
    pub bp: Vec<i64>,
    pub cm: Vec<f64>,
    /// Original `.bim` indices of retained SNPs (into the full SNP list).
    pub kept_snps: Vec<usize>,
    /// Major-allele frequency of each retained SNP.
    pub freq: Vec<f64>,
    /// `min(freq, 1-freq)`.
    pub maf: Vec<f64>,
    /// `sqrt(freq*(1-freq))`.
    pub sqrtpq: Vec<f64>,
    /// Genotype matrix, `m × n`, values in {0,1,2,9} (9 = missing). Row = SNP,
    /// in retained order; column = retained individual.
    geno: Vec<Vec<f64>>,
    next_snp: usize,
}

impl PlinkBed {
    /// Read a `.bed` file. Port of `PlinkBEDFile.__init__`.
    ///
    /// * `path` — the `.bed` filename (must end in `.bed`).
    /// * `n` — number of individuals in the `.fam`.
    /// * `bim` — the matching `.bim` (provides CHR/SNP/BP/CM and the SNP count).
    /// * `keep_snps` — optional indices (into the full SNP list) to restrict to.
    /// * `keep_indivs` — optional indices (into the `.fam` individuals) to keep.
    /// * `maf_min` — minimum MAF; SNPs with `maf <= maf_min` are dropped
    ///   (Python default `0`, which drops monomorphic SNPs).
    pub fn read(
        path: &str,
        n: usize,
        bim: &BimFile,
        keep_snps: Option<&[usize]>,
        keep_indivs: Option<&[usize]>,
        maf_min: f64,
    ) -> Result<Self> {
        if !path.ends_with(".bed") {
            return Err(LdscError::Plink(".bed filename must end in .bed".into()));
        }
        let bytes = std::fs::read(path)?;
        if bytes.len() < 3 {
            return Err(LdscError::Plink(".bed file too short for header".into()));
        }
        if bytes[0] != 0x6c || bytes[1] != 0x1b {
            return Err(LdscError::Plink(
                "Magic number from Plink .bed file not recognized".into(),
            ));
        }
        if bytes[2] != 0x01 {
            return Err(LdscError::Plink(
                "Plink .bed file must be in default SNP-major mode".into(),
            ));
        }

        let m_total = bim.snp.len();
        let indiv_idx: Vec<usize> = match keep_indivs {
            Some(k) => {
                for &i in k {
                    if i >= n {
                        return Err(LdscError::Plink("keep_indivs indices out of bounds".into()));
                    }
                }
                k.to_vec()
            }
            None => (0..n).collect(),
        };
        let n_keep = indiv_idx.len();
        if n_keep == 0 {
            return Err(LdscError::Plink(
                "After filtering, no individuals remain".into(),
            ));
        }

        let nru_full = nru(n);
        // Per-individual byte/bit location in the full (unfiltered) layout.
        // geno bytes for SNP s start at offset 3 + s*bytes_per_snp.
        let bytes_per_snp = nru_full / 4;
        if bytes.len() - 3 != m_total * bytes_per_snp {
            return Err(LdscError::Plink(format!(
                "Plink .bed file has {} bytes of genotype data, expected {}",
                bytes.len() - 3,
                m_total * bytes_per_snp
            )));
        }

        // Decode all SNPs × kept individuals into geno0 (m_total × n_keep).
        let mut geno0: Vec<Vec<f64>> = Vec::with_capacity(m_total);
        for s in 0..m_total {
            let off = 3 + s * bytes_per_snp;
            let row: Vec<f64> = indiv_idx
                .iter()
                .map(|&j| {
                    let b = bytes[off + j / 4];
                    let code = (b >> (2 * (j % 4))) & 0b11;
                    decode_code(code)
                })
                .collect();
            geno0.push(row);
        }

        // --- filter_snps_maf ---
        let considered: Vec<usize> = match keep_snps {
            Some(k) => {
                for &j in k {
                    if j >= m_total {
                        return Err(LdscError::Plink("keep_snps indices out of bounds".into()));
                    }
                }
                k.to_vec()
            }
            None => (0..m_total).collect(),
        };

        let mut kept_snps = Vec::new();
        let mut freq = Vec::new();
        let mut geno = Vec::new();
        let maf_min = if maf_min > 0.0 { maf_min } else { 0.0 };
        for &j in &considered {
            let row = &geno0[j];
            // bit-counting MAF filter (port of __filter_snps_maf__).
            // a = missing + hom(code3) , b = het + hom , c = hom(code3).
            let (mut a, mut b, mut c) = (0u64, 0u64, 0u64);
            for &g in row {
                let code = if (g - 9.0).abs() < 0.5 {
                    0b10 // missing
                } else {
                    // map genotype value back to raw code: 0->0,1->1(0b01),2->3(0b11)
                    match g as i64 {
                        0 => 0b00,
                        1 => 0b01,
                        2 => 0b11,
                        _ => 0b00,
                    }
                };
                let bit0 = code & 1;
                let bit1 = (code >> 1) & 1;
                a += (bit0) as u64;
                b += (bit1) as u64;
                c += (bit0 & bit1) as u64;
            }
            let major_ct = b + c; // = het + 2*hom
            let n_nomiss = (n_keep as i64) - (a as i64) + (c as i64);
            let f = if n_nomiss > 0 {
                major_ct as f64 / (2.0 * n_nomiss as f64)
            } else {
                0.0
            };
            let het_miss_ct = a + b - 2 * c; // = missing + het
            let maf = f.min(1.0 - f);
            if maf > maf_min && (het_miss_ct as usize) < n_keep {
                kept_snps.push(j);
                freq.push(f);
                geno.push(row.clone());
            }
        }
        let m = kept_snps.len();
        if m == 0 {
            return Err(LdscError::Plink("After filtering, no SNPs remain".into()));
        }

        // df filtered to kept SNPs.
        let chr = kept_snps.iter().map(|&j| bim.chr[j]).collect();
        let snp = kept_snps.iter().map(|&j| bim.snp[j].clone()).collect();
        let bp = kept_snps.iter().map(|&j| bim.bp[j]).collect();
        let cm = kept_snps.iter().map(|&j| bim.cm[j]).collect();
        let maf: Vec<f64> = freq.iter().map(|&f| f.min(1.0 - f)).collect();
        let sqrtpq: Vec<f64> = freq.iter().map(|&f| (f * (1.0 - f)).sqrt()).collect();

        Ok(PlinkBed {
            m,
            n: n_keep,
            chr,
            snp,
            bp,
            cm,
            kept_snps,
            freq,
            maf,
            sqrtpq,
            geno,
            next_snp: 0,
        })
    }

    /// Column names of the metadata table LDSC attaches (`CHR SNP BP CM MAF`).
    pub fn colnames() -> &'static [&'static str] {
        &["CHR", "SNP", "BP", "CM", "MAF"]
    }

    /// Unbiased L² correction: `sq - (1-sq)/(n-2)` (n>2 else n). Port of
    /// `__l2_unbiased__`.
    pub fn l2_unbiased(sq: f64, n: usize) -> f64 {
        let denom = if n > 2 { (n - 2) as f64 } else { n as f64 };
        sq - (1.0 - sq) / denom
    }

    /// `nextSNPs(b, minor_ref)` — the next `b` SNPs as an `n × b` matrix of
    /// standardized genotypes (mean 0, variance 1 per SNP; missing imputed to
    /// the mean). With `minor_ref`, flips the sign so the minor allele is
    /// positive when its frequency exceeds 0.5.
    pub fn next_snps(&mut self, b: usize, minor_ref: bool) -> Result<Vec<Vec<f64>>> {
        if b == 0 {
            return Err(LdscError::InvalidInput("b must be > 0".into()));
        }
        if self.next_snp + b > self.m {
            return Err(LdscError::InvalidInput(format!(
                "{b} SNPs requested, {} SNPs remain",
                self.m - self.next_snp
            )));
        }
        let n = self.n;
        // result is n × b (row = individual, col = SNP).
        let mut out = vec![vec![0.0; b]; n];
        for jj in 0..b {
            let s = self.next_snp + jj;
            let col = &self.geno[s];
            // mean of non-missing
            let (sum, cnt) = col
                .iter()
                .filter(|&&g| (g - 9.0).abs() > 0.5)
                .fold((0.0, 0usize), |(s, c), &g| (s + g, c + 1));
            let avg = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
            // impute missing → avg, then compute std (population, ddof=0 like np.std)
            let imputed: Vec<f64> = col
                .iter()
                .map(|&g| if (g - 9.0).abs() < 0.5 { avg } else { g })
                .collect();
            let mean = imputed.iter().sum::<f64>() / n as f64;
            let var: f64 = imputed.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
            let mut denom = var.sqrt();
            if denom == 0.0 {
                denom = 1.0;
            }
            if minor_ref && self.freq[s] > 0.5 {
                denom = -denom;
            }
            for i in 0..n {
                out[i][jj] = (imputed[i] - avg) / denom;
            }
        }
        self.next_snp += b;
        Ok(out)
    }

    /// Reset the sequential SNP cursor (used by tests / re-iteration).
    pub fn reset_cursor(&mut self) {
        self.next_snp = 0;
    }

    /// Borrow the raw kept genotype row for a SNP (values 0/1/2/9).
    pub fn geno_row(&self, s: usize) -> &[f64] {
        &self.geno[s]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BimFile;

    fn data(p: &str) -> String {
        format!("tests/data/{p}")
    }

    fn read_bed(keep_snps: Option<&[usize]>, keep_indivs: Option<&[usize]>) -> PlinkBed {
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        PlinkBed::read(
            &data("plink_test/plink.bed"),
            5,
            &bim,
            keep_snps,
            keep_indivs,
            0.0,
        )
        .unwrap()
    }

    #[test]
    fn bed_filters_to_4_polymorphic() {
        // test_ldscore.test_bed: 8 SNPs, 3 monomorphic removed → m=4, n=5.
        let bed = read_bed(None, None);
        assert_eq!(bed.m, 4);
        assert_eq!(bed.n, 5);
        // freq == [0.6, 0.6, 0.625, 0.625]
        let freq: Vec<f64> = bed.freq.iter().map(|&f| (f * 1e8).round() / 1e8).collect();
        assert_eq!(freq, vec![0.6, 0.6, 0.625, 0.625], "freq = {:?}", bed.freq);
    }

    #[test]
    fn filter_snps_keeps_subset() {
        // test_filter_snps: keep_snps=[1,4] → only 1 polymorphic remains.
        let bed = read_bed(Some(&[1, 4]), None);
        assert_eq!(bed.m, 1);
        assert_eq!(bed.n, 5);
        assert_eq!(bed.kept_snps, vec![4]); // SNP 4 is polymorphic among {1,4}
    }

    #[test]
    fn filter_indivs() {
        // test_filter_indivs: keep [0,1] → m=2 polymorphic, n=2.
        let bed = read_bed(None, Some(&[0, 1]));
        assert_eq!(bed.m, 2);
        assert_eq!(bed.n, 2);
    }

    #[test]
    fn filter_indivs_and_snps() {
        // test_filter_indivs_and_snps: keep_indivs=[0,1], keep_snps=[1,5] → m=1.
        let bed = read_bed(Some(&[1, 5]), Some(&[0, 1]));
        assert_eq!(bed.m, 1);
        assert_eq!(bed.n, 2);
    }

    #[test]
    fn bad_filename_errors() {
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        assert!(PlinkBed::read(&data("plink_test/plink.bim"), 9, &bim, None, None, 0.0).is_err());
    }

    #[test]
    fn next_snps_errors() {
        let mut bed = read_bed(None, None);
        assert!(bed.next_snps(0, false).is_err());
        assert!(bed.next_snps(5, false).is_err()); // only 4 SNPs
    }

    #[test]
    fn next_snps_shape_and_standardized() {
        // test_nextSNPs: shape (5,b); mean≈0, std≈1 per SNP.
        let mut bed = read_bed(None, None);
        for b in [1usize, 2, 3] {
            let mut bed_b = read_bed(None, None);
            let x = bed_b.next_snps(b, false).unwrap();
            assert_eq!(x.len(), 5); // n rows
            assert!(x.iter().all(|r| r.len() == b));
            for jj in 0..b {
                let mean: f64 = x.iter().map(|r| r[jj]).sum::<f64>() / 5.0;
                let var: f64 = x.iter().map(|r| (r[jj] - mean).powi(2)).sum::<f64>() / 5.0;
                assert!(mean.abs() < 0.01, "mean[{jj}]={mean}");
                assert!((var.sqrt() - 1.0).abs() < 0.01, "std[{jj}]={}", var.sqrt());
            }
            bed.reset_cursor();
        }
    }

    #[test]
    fn next_snps_minor_ref_flips_sign() {
        // test_nextSNPs_maf_ref: x == -y when minor_ref is set.
        let mut bed = read_bed(None, None);
        let b = 4;
        let x = bed.next_snps(b, false).unwrap();
        bed.reset_cursor();
        let y = bed.next_snps(b, true).unwrap();
        for i in 0..5 {
            for jj in 0..b {
                assert!((x[i][jj] + y[i][jj]).abs() < 1e-9, "sign flip mismatch");
            }
        }
    }

    #[test]
    fn l2_unbiased_formula() {
        // sq - (1-sq)/(n-2)
        assert!((PlinkBed::l2_unbiased(0.5, 10) - (0.5 - 0.5 / 8.0)).abs() < 1e-12);
    }
}
