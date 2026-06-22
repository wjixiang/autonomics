//! Internal numeric helpers shared across modules.

/// Compensated (Kahan–Babuška / Neumaier) summation.
///
/// Plain running sums accumulate catastrophic rounding error when adding many
/// values of differing magnitude — exactly the case in variance/covariance
/// computations. Neumaier's improvement over classic Kahan also handles the
/// case where the addend is larger than the running sum.
///
/// This is an internal primitive; the public API returns plain `f64`.
pub(crate) fn compensated_sum(iter: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0_f64;
    let mut c = 0.0_f64; // compensation
    for value in iter {
        let t = sum + value;
        // Neumaier correction: if the magnitude of `sum` dominates, account for
        // the low-order bits lost from `value`, and vice versa.
        if sum.abs() >= value.abs() {
            c += (sum - t) + value;
        } else {
            c += (value - t) + sum;
        }
        sum = t;
    }
    sum + c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compensated_sum_matches_naive_for_small_inputs() {
        assert_eq!(compensated_sum([1.0, 2.0, 3.0, 4.0, 5.0]), 15.0);
    }

    #[test]
    fn compensated_sum_is_more_accurate_than_naive() {
        // A classic case where naive summation loses precision: a large value
        // followed by many tiny ones.
        let large = 1e16_f64;
        let tiny = 1.0_f64;
        let values = [large, tiny, tiny, tiny];

        let naive: f64 = values.iter().sum();
        let comp = compensated_sum(values);

        // Naive sum drops the three unit addends entirely; compensated keeps them.
        assert_eq!(naive, large, "naive sum loses the small terms");
        assert_eq!(comp, large + 3.0);
    }
}
