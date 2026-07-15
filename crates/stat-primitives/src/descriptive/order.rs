//! Order statistics: min, max, k-th order statistic, and ranks.
//!
//! Ranks use the **average** method for ties (each tied observation receives
//! the mean of the ranks it spans), matching R's default and SciPy's
//! `'average'`. This is the foundation for nonparametric methods (Spearman,
//! Wilcoxon) added in later layers.

use crate::error::{Result, StatError};

/// Minimum value.
pub fn min(xs: &[f64]) -> Result<f64> {
    xs.iter()
        .copied()
        .min_by(|a, b| a.total_cmp(b))
        .ok_or(StatError::EmptyInput)
}

/// Maximum value.
pub fn max(xs: &[f64]) -> Result<f64> {
    xs.iter()
        .copied()
        .max_by(|a, b| a.total_cmp(b))
        .ok_or(StatError::EmptyInput)
}

/// The `k`-th smallest value, 1-indexed (`k = 1` → minimum, `k = n` → maximum).
///
/// Works on a copy so the caller's data is untouched; uses partial selection so
/// it is O(n) on average rather than O(n log n).
pub fn order_statistic(xs: &[f64], k: usize) -> Result<f64> {
    let n = xs.len();
    if n == 0 {
        return Err(StatError::EmptyInput);
    }
    if k == 0 || k > n {
        return Err(StatError::InvalidInput(format!(
            "order statistic index {k} out of range 1..={n}"
        )));
    }
    let mut sorted = xs.to_vec();
    // select_nth_unstable_by partitions so that index `k-1` holds the k-th
    // order statistic (in sorted order) — exactly what we want, without a full
    // sort. We use `_by` because `f64` is not `Ord` (NaN has no total order).
    let (before, elt, _after) = sorted.select_nth_unstable_by(k - 1, |a, b| a.total_cmp(b));
    let _ = before;
    Ok(*elt)
}

/// Average ranks of the values (ties share the mean rank).
///
/// Returns a vector the same length as the input; rank 1 is the smallest.
pub fn rank(xs: &[f64]) -> Result<Vec<f64>> {
    if xs.is_empty() {
        return Err(StatError::EmptyInput);
    }
    // Sort indices by value, then walk runs of equal values assigning the
    // average of the ranks they span (1-indexed).
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    idx.sort_by(|&a, &b| xs[a].total_cmp(&xs[b]));

    let mut ranks = vec![0.0_f64; xs.len()];
    let mut i = 0;
    while i < idx.len() {
        // Find the end of the tie group starting at i.
        let mut j = i + 1;
        while j < idx.len() && xs[idx[j]] == xs[idx[i]] {
            j += 1;
        }
        // Average rank of positions i+1 .. j (1-indexed).
        let avg = ((i + 1) + j) as f64 / 2.0;
        for &k in &idx[i..j] {
            ranks[k] = avg;
        }
        i = j;
    }
    Ok(ranks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_basic() {
        let xs = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        assert_eq!(min(&xs).unwrap(), 1.0);
        assert_eq!(max(&xs).unwrap(), 9.0);
    }

    #[test]
    fn order_statistic_picks_correct_kth() {
        let xs = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        assert_eq!(order_statistic(&xs, 1).unwrap(), 1.0); // min
        assert_eq!(order_statistic(&xs, 8).unwrap(), 9.0); // max
        assert_eq!(order_statistic(&xs, 4).unwrap(), 3.0);
        // Original slice unchanged.
        assert_eq!(xs[0], 3.0);
    }

    #[test]
    fn order_statistic_validation() {
        let xs = [1.0, 2.0];
        assert!(matches!(
            order_statistic(&xs, 0),
            Err(StatError::InvalidInput(_))
        ));
        assert!(matches!(
            order_statistic(&xs, 3),
            Err(StatError::InvalidInput(_))
        ));
        assert!(matches!(
            order_statistic(&[], 1),
            Err(StatError::EmptyInput)
        ));
    }

    #[test]
    fn rank_average_ties() {
        // [1, 2, 2, 3]: ranks 1, (2+3)/2=2.5, 2.5, 4.
        let xs = [1.0, 2.0, 2.0, 3.0];
        let r = rank(&xs).unwrap();
        assert_eq!(r, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn rank_no_ties() {
        let xs = [3.0, 1.0, 2.0];
        assert_eq!(rank(&xs).unwrap(), vec![3.0, 1.0, 2.0]);
    }

    #[test]
    fn rank_nan_propagates_to_end() {
        // total_cmp orders NaN as largest; we just assert it does not panic and
        // produces the right length (NaN handling is best-effort here).
        let xs = [1.0, f64::NAN, 2.0];
        let r = rank(&xs).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], 1.0);
    }
}
