//! Pure-Rust BED interval engine — a self-contained replacement for the
//! `pybedtools` calls in `make_annot.py` (sort, merge, point-overlap). No
//! external `bedtools` binary required.
//!
//! Coordinates are **1-based inclusive** internally (the natural form for SNP
//! base-pair positions). BED files (0-based half-open) are converted on read.

use std::collections::HashMap;

use crate::io::read_lines;
use crate::{LdscError, Result};

/// A genomic interval, 1-based inclusive `[start, end]` on `chrom`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub chrom: String,
    pub start: i64,
    pub end: i64,
}

impl Interval {
    /// From a BED row (0-based half-open `[start, end)`), convert to 1-based
    /// inclusive `[start+1, end]`.
    pub fn from_bed(chrom: &str, start: i64, end: i64) -> Self {
        Interval {
            chrom: chrom.to_string(),
            start: start + 1,
            end,
        }
    }

    /// Does the interval contain the 1-based position `pos`?
    pub fn contains(&self, pos: i64) -> bool {
        self.start <= pos && pos <= self.end
    }
}

/// Merge overlapping intervals per chromosome (port of `bedtools merge`).
/// Input order is irrelevant; output is sorted by `(chrom, start)`.
pub fn merge(intervals: Vec<Interval>) -> Vec<Interval> {
    let mut by_chrom: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    for iv in intervals {
        by_chrom
            .entry(iv.chrom.clone())
            .or_default()
            .push((iv.start, iv.end));
    }
    let mut chroms: Vec<String> = by_chrom.keys().cloned().collect();
    chroms.sort();
    let mut out = Vec::new();
    for c in chroms {
        let mut v = by_chrom.remove(&c).unwrap();
        v.sort();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (s, e) in v {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 + 1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        for (s, e) in merged {
            out.push(Interval {
                chrom: c.clone(),
                start: s,
                end: e,
            });
        }
    }
    out
}

/// Count how many of `intervals` (on `chrom`) contain `pos`. Used for the
/// `--nomerge` annotation (annot = overlap count).
pub fn count_overlaps(intervals: &[Interval], chrom: &str, pos: i64) -> usize {
    intervals
        .iter()
        .filter(|iv| iv.chrom == chrom && iv.contains(pos))
        .count()
}

/// Does any interval (on `chrom`) contain `pos`?
pub fn any_overlap(intervals: &[Interval], chrom: &str, pos: i64) -> bool {
    intervals
        .iter()
        .any(|iv| iv.chrom == chrom && iv.contains(pos))
}

/// Read a UCSC BED file (columns: chrom, start, end, …) into 1-based inclusive
/// intervals. `min_cols` fields are required (default 3).
pub fn read_bed(path: &str) -> Result<Vec<Interval>> {
    let lines = read_lines(path)?;
    let mut out = Vec::new();
    for line in lines.iter() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("track") || t.starts_with("browser")
        {
            continue;
        }
        let f: Vec<&str> = t.split_whitespace().collect();
        if f.len() < 3 {
            return Err(LdscError::Parse {
                context: "bed".into(),
                reason: format!("BED line needs ≥3 columns: {line}"),
            });
        }
        let chrom = f[0].to_string();
        let start: i64 = f[1].parse().map_err(|_| LdscError::Parse {
            context: "bed".into(),
            reason: format!("bad start: {}", f[1]),
        })?;
        let end: i64 = f[2].parse().map_err(|_| LdscError::Parse {
            context: "bed".into(),
            reason: format!("bad end: {}", f[2]),
        })?;
        out.push(Interval::from_bed(&chrom, start, end));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_overlaps_per_chrom() {
        let ivs = vec![
            Interval {
                chrom: "1".into(),
                start: 100,
                end: 200,
            },
            Interval {
                chrom: "1".into(),
                start: 150,
                end: 250,
            }, // overlaps → merge to 100-250
            Interval {
                chrom: "1".into(),
                start: 300,
                end: 400,
            }, // disjoint
            Interval {
                chrom: "2".into(),
                start: 10,
                end: 20,
            },
        ];
        let m = merge(ivs);
        assert_eq!(m.len(), 3);
        assert!(m.contains(&Interval {
            chrom: "1".into(),
            start: 100,
            end: 250
        }));
        assert!(m.contains(&Interval {
            chrom: "1".into(),
            start: 300,
            end: 400
        }));
        assert!(m.contains(&Interval {
            chrom: "2".into(),
            start: 10,
            end: 20
        }));
    }

    #[test]
    fn bed_from_half_open() {
        // BED [50, 100) (0-based) → 1-based inclusive [51, 100]
        let iv = Interval::from_bed("1", 50, 100);
        assert_eq!(iv.start, 51);
        assert_eq!(iv.end, 100);
        assert!(iv.contains(51));
        assert!(iv.contains(100));
        assert!(!iv.contains(50));
        assert!(!iv.contains(101));
    }

    #[test]
    fn count_and_any_overlap() {
        let ivs = vec![
            Interval {
                chrom: "1".into(),
                start: 100,
                end: 200,
            },
            Interval {
                chrom: "1".into(),
                start: 150,
                end: 250,
            },
        ];
        assert_eq!(count_overlaps(&ivs, "1", 175), 2); // in both
        assert_eq!(count_overlaps(&ivs, "1", 125), 1); // only first
        assert_eq!(count_overlaps(&ivs, "1", 50), 0);
        assert!(any_overlap(&ivs, "1", 175));
        assert!(!any_overlap(&ivs, "2", 175));
    }
}
