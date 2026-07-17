//! Allele tables and strand / reference-allele flip logic — a faithful port of
//! the module-level constants in `ldscore/sumstats.py:24-48`.
//!
//! LDSC matches alleles between two sumstats files (or against `--merge-alleles`)
//! allowing for a strand flip and/or a reference-allele flip. The combinatorics
//! are expressed here as pure functions over the 2- and 4-character allele
//! strings the Python code uses (`A1+A2` for a SNP, `A1+A2+A1x+A2x` for a pair).
//!
//! - [`is_valid_snp`] — `VALID_SNPS`: a 2-char pair is a biallelic,
//!   strand-unambiguous SNP (two distinct ACGT bases that are not complements).
//! - [`alleles_match`] — `MATCH_ALLELES`: two SNPs have the same alleles,
//!   allowing strand and/or reference flip.
//! - [`flip_alleles`] — `FLIP_ALLELES`: whether a reference-allele flip is
//!   needed to align the second SNP's effect direction to the first.

const BASES: &[u8] = b"ATCG";

#[inline]
fn is_base(b: u8) -> bool {
    matches!(b, b'A' | b'T' | b'C' | b'G')
}

/// Complement of a single base (`A↔T`, `C↔G`); non-ACGT passes through unchanged.
/// Port of `COMPLEMENT`.
#[inline]
pub fn complement_base(base: char) -> char {
    complement(base as u8) as char
}

#[inline]
fn complement(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'T' => b'A',
        b'C' => b'G',
        b'G' => b'C',
        other => other,
    }
}

#[inline]
fn strand_ambiguous(a: u8, b: u8) -> bool {
    a != b && a == complement(b)
}

/// `VALID_SNPS`: is `pair` (e.g. `"AC"`) a strand-unambiguous biallelic SNP?
/// Used by munge's `filter_alleles(A1+A2)`.
pub fn is_valid_snp(pair: &str) -> bool {
    let b = pair.as_bytes();
    b.len() == 2 && b[0] != b[1] && is_base(b[0]) && is_base(b[1]) && !strand_ambiguous(b[0], b[1])
}

#[inline]
fn valid_pair(a: u8, c: u8) -> bool {
    a != c && is_base(a) && is_base(c) && !strand_ambiguous(a, c)
}

/// `MATCH_ALLELES`: do two SNPs have the same alleles, allowing strand and/or
/// reference flip? `four` = `A1+A2+A1x+A2x` (the two 2-char allele pairs
/// concatenated).
pub fn alleles_match(four: &str) -> bool {
    let b = four.as_bytes();
    if b.len() != 4 {
        return false;
    }
    let (a, c, d, e) = (b[0], b[1], b[2], b[3]); // s1 = a|c, s2 = d|e
    if !(valid_pair(a, c) && valid_pair(d, e)) {
        return false;
    }
    // strand+ref match | ref match+strand flip | ref flip+strand match | both flip
    (a == d && c == e)
        || (a == complement(d) && c == complement(e))
        || (a == e && c == d)
        || (a == complement(e) && c == complement(d))
}

/// `FLIP_ALLELES`: does aligning the second SNP to the first require negating
/// its effect (a reference-allele flip)? `four` = `A1+A2+A1x+A2x`.
///
/// True for a reference flip (strand match or strand flip). Only meaningful for
/// `four` values for which [`alleles_match`] is true (callers filter first).
pub fn flip_alleles(four: &str) -> bool {
    let b = four.as_bytes();
    if b.len() != 4 {
        return false;
    }
    let (a, c, d, e) = (b[0], b[1], b[2], b[3]);
    (a == e && c == d) || (a == complement(e) && c == complement(d))
}

/// The eight `VALID_SNPS` ("AC","AG","CA","CT","GA","GT","TC","TG"), for tests.
pub fn valid_snps() -> Vec<String> {
    let mut out = Vec::new();
    for &b1 in BASES {
        for &b2 in BASES {
            let p = [b1, b2];
            if is_valid_snp(std::str::from_utf8(&p).unwrap()) {
                out.push(String::from_utf8(p.to_vec()).unwrap());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_snps_match_python() {
        // sumstats.VALID_SNPS — the 8 strand-unambiguous biallelic SNPs (a set).
        let mut v = valid_snps();
        v.sort();
        assert_eq!(
            v,
            vec![
                "AC".to_string(),
                "AG".into(),
                "CA".into(),
                "CT".into(),
                "GA".into(),
                "GT".into(),
                "TC".into(),
                "TG".into()
            ]
        );
    }

    #[test]
    fn filter_alleles_matches_test() {
        // test_munge_sumstats.test_filter_alleles: first 8 valid, rest invalid.
        let a = [
            "AC", "AG", "CA", "CT", "GA", "GT", "TC", "TG", "DI", "AAT", "RA",
        ];
        let expected = [
            true, true, true, true, true, true, true, true, false, false, false,
        ];
        for (x, exp) in a.iter().zip(expected.iter()) {
            assert_eq!(is_valid_snp(x), *exp, "{x}");
        }
    }

    #[test]
    fn strand_ambiguous_pairs_excluded() {
        // AT/TA/CG/GC are strand-ambiguous → not valid.
        assert!(!is_valid_snp("AT"));
        assert!(!is_valid_snp("TA"));
        assert!(!is_valid_snp("CG"));
        assert!(!is_valid_snp("GC"));
    }

    #[test]
    fn match_and_flip_semantics() {
        // Same alleles, same order → match, no flip.
        assert!(alleles_match("ACAC"));
        assert!(!flip_alleles("ACAC"));
        // Reference flip (A1/A2 swapped in second) → match, flip.
        assert!(alleles_match("ACCA"));
        assert!(flip_alleles("ACCA"));
        // Strand flip (complements) → match, no flip.
        assert!(alleles_match("ACTG")); // AC vs TG (complements)
        assert!(!flip_alleles("ACTG"));
        // Strand + ref flip → match, flip.
        assert!(alleles_match("ACGT"));
        assert!(flip_alleles("ACGT"));
        // Different alleles → no match.
        assert!(!alleles_match("ACAG"));
        // Invalid pair → no match.
        assert!(!alleles_match("ATCA")); // AT ambiguous
    }
}
