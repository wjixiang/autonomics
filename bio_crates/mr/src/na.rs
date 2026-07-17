//! NA / validity helpers matching R's `is.na` semantics.
//!
//! R represents missing values as `NA_real_`; arithmetic with `NA` propagates
//! `NA`. We use `f64::NAN` as the in-band sentinel. R's `is.na()` returns
//! `TRUE` for **both** `NA` and `NaN` (but `FALSE` for `Inf`/`-Inf`), so a
//! value is "valid" iff it is not NaN. We deliberately treat infinities as
//! valid to match R — e.g. `is.na(Inf)` is `FALSE`.

/// A value is valid (non-NA) iff it is not NaN. `±Inf` count as valid,
/// matching R's `is.na`.
#[inline]
pub fn is_valid(x: &f64) -> bool {
    !x.is_nan()
}

/// R's `!is.na(x)` over a slice — count of non-NA entries.
pub fn count_valid(x: &[f64]) -> usize {
    x.iter().filter(|v| is_valid(v)).count()
}

/// R's `sum(!is.na(x) & !is.na(y))` — pairwise valid count.
pub fn count_valid2(x: &[f64], y: &[f64]) -> usize {
    x.iter()
        .zip(y.iter())
        .filter(|(a, b)| is_valid(a) && is_valid(b))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_semantics() {
        assert!(is_valid(&1.0));
        assert!(is_valid(&f64::INFINITY));
        assert!(is_valid(&f64::NEG_INFINITY));
        assert!(!is_valid(&f64::NAN));
        assert_eq!(count_valid(&[1.0, f64::NAN, 3.0, f64::INFINITY]), 3);
        assert_eq!(
            count_valid2(&[1.0, f64::NAN, 3.0], &[f64::NAN, f64::NAN, 9.0]),
            1
        );
    }
}
