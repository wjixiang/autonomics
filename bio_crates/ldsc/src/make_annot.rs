//! Annotation building — a pure-Rust port of `make_annot.py`.
//!
//! Produces a `.annot` file (one `0/1`-per-SNP column) from either
//! - a gene-set file + gene-coordinate file (+ window), or
//! - a UCSC BED file of annotation regions,
//! scored against the SNPs in a PLINK `.bim`. Replaces `pybedtools` with the
//! pure-Rust [`crate::bed`] engine.

use crate::bed::{Interval, any_overlap, count_overlaps, merge, read_bed};
use crate::io::{BimFile, read_lines};
use crate::{LdscError, Result};

/// Build the annotation from a gene set + gene coordinates + window.
/// Port of `gene_set_to_bed` + `make_annot_files`.
///
/// `gene_set_path`: one gene name per line. `gene_coord_path`: whitespace
/// `GENE CHR START END`. Returns one annotation value per `.bim` SNP.
pub fn gene_set_to_annot(
    gene_set_path: &str,
    gene_coord_path: &str,
    windowsize: i64,
    bim: &BimFile,
) -> Result<Vec<i64>> {
    let gene_set: std::collections::HashSet<String> = read_lines(gene_set_path)?
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let coord_lines = read_lines(gene_coord_path)?;
    let mut intervals = Vec::new();
    for line in &coord_lines {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let gene = f[0];
        if !gene_set.contains(gene) {
            continue;
        }
        let chrom = strip_chr(f[1]);
        let start: i64 = f[2].parse().map_err(|_| LdscError::Parse {
            context: "gene_coord".into(),
            reason: format!("bad START: {}", f[2]),
        })?;
        let end: i64 = f[3].parse().map_err(|_| LdscError::Parse {
            context: "gene_coord".into(),
            reason: format!("bad END: {}", f[3]),
        })?;
        // window: [max(1, START-ws), END+ws] (1-based inclusive)
        let s = (start - windowsize).max(1);
        let e = end + windowsize;
        intervals.push(Interval {
            chrom,
            start: s,
            end: e,
        });
    }
    let merged = merge(intervals);
    Ok(bim
        .chr
        .iter()
        .zip(bim.bp.iter())
        .map(|(chrom, bp)| any_overlap(&merged, &chrom.to_string(), *bp) as i64)
        .collect())
}

/// Build the annotation from a BED file of regions. Port of the `--bed-file`
/// branch. With `nomerge`, the annotation is the count of overlapping intervals
/// (so a SNP covered twice gets value 2); otherwise merged → 0/1.
pub fn bed_to_annot(bed_path: &str, bim: &BimFile, nomerge: bool) -> Result<Vec<i64>> {
    let ivs = read_bed(bed_path)?;
    let intervals = if nomerge { ivs } else { merge(ivs) };
    Ok(bim
        .chr
        .iter()
        .zip(bim.bp.iter())
        .map(|(chrom, bp)| {
            if nomerge {
                count_overlaps(&intervals, &chrom.to_string(), *bp) as i64
            } else {
                any_overlap(&intervals, &chrom.to_string(), *bp) as i64
            }
        })
        .collect())
}

/// Write a single-column `.annot` file (header `ANNOT`).
pub fn write_annot(path: &str, annot: &[i64]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "ANNOT")?;
    for v in annot {
        writeln!(f, "{v}")?;
    }
    Ok(())
}

fn strip_chr(s: &str) -> String {
    // match make_annot.py: 'chr'+str(x).lstrip('chr')
    s.strip_prefix("chr").unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn data(p: &str) -> String {
        format!("tests/data/{p}")
    }

    fn tmp(p: &str) -> String {
        let t = std::env::temp_dir().join(p);
        t.to_string_lossy().into_owned()
    }

    #[test]
    fn gene_set_roundtrip() {
        // Synthetic: gene G1 chr1 [100,200], window 50 → [50,250]. gene G2 chr1 [500,600] window 0.
        let gene_set = tmp("ldsc_geneset.txt");
        let gene_coord = tmp("ldsc_genecoord.txt");
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        // write gene set
        {
            let mut f = std::fs::File::create(&gene_set).unwrap();
            writeln!(f, "G1").unwrap();
        }
        {
            let mut f = std::fs::File::create(&gene_coord).unwrap();
            writeln!(f, "G1\t1\t100\t200").unwrap();
        }
        let annot = gene_set_to_annot(&gene_set, &gene_coord, 50, &bim).unwrap();
        // bim BPs are 1..8 (rs_0..rs_7 at BP 1..8). Window [50,250] contains none.
        assert!(annot.iter().all(|v| *v == 0), "{annot:?}");

        // Now a window large enough to cover BP 1..8: G1 chr1 [100,200], ws=200 → [-100→1, 400]
        let annot = gene_set_to_annot(&gene_set, &gene_coord, 200, &bim).unwrap();
        // window [1,400] covers BP 1..8
        assert!(annot.iter().all(|v| *v == 1), "{annot:?}");
    }

    #[test]
    fn bed_to_annot_merged_and_nomerge() {
        let bim = BimFile::read(&data("plink_test/plink.bim")).unwrap();
        // BED covering BP 1..4 (0-based [0,4)) on chr1.
        let bed = tmp("ldsc_annot.bed");
        {
            let mut f = std::fs::File::create(&bed).unwrap();
            writeln!(f, "1\t0\t4").unwrap(); // 1-based [1,4]
        }
        let merged = bed_to_annot(&bed, &bim, false).unwrap();
        // SNPs at BP 1,2,3,4 → 1; BP 5,6,7,8 → 0
        assert_eq!(merged, vec![1, 1, 1, 1, 0, 0, 0, 0]);

        // nomerge with two identical intervals → SNPs 1..4 get count 2.
        {
            let mut f = std::fs::File::create(&bed).unwrap();
            writeln!(f, "1\t0\t4").unwrap();
            writeln!(f, "1\t0\t4").unwrap();
        }
        let counted = bed_to_annot(&bed, &bim, true).unwrap();
        assert_eq!(counted, vec![2, 2, 2, 2, 0, 0, 0, 0]);
    }
}
