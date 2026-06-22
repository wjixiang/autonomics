//! The [`Distribution`] trait — a uniform `pdf` / `cdf` / `sf` / `ppf` surface.

/// A probability distribution.
///
/// `pdf` is the density, `cdf` the cumulative distribution `P(X ≤ x)`, `sf` the
/// survival function `P(X > x) = 1 − cdf`, and `ppf` the quantile (inverse CDF).
pub trait Distribution {
    /// Probability density at `x`.
    fn pdf(&self, x: f64) -> f64;

    /// Cumulative probability `P(X ≤ x)`.
    fn cdf(&self, x: f64) -> f64;

    /// Survival function `P(X > x) = 1 − cdf(x)`.
    ///
    /// Default implementation is `1.0 - cdf(x)`; distributions may override
    /// this with a more accurate tail computation.
    fn sf(&self, x: f64) -> f64 {
        1.0 - self.cdf(x)
    }

    /// Quantile function (inverse CDF): the smallest `x` with `cdf(x) ≥ p`,
    /// for `p ∈ (0, 1)`.
    fn ppf(&self, p: f64) -> f64;
}
