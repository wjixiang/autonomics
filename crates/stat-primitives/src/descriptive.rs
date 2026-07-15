//! Layer 1 — descriptive statistics.
//!
//! Split into unweighted ([`summarize`]), weighted ([`weighted`]), and
//! order ([`order`]) statistics. All operate on borrowed slices; none allocate
//! except where sorting is unavoidable ([`quantile`], [`rank`], order
//! statistics).

pub mod order;
pub mod summarize;
pub mod weighted;

pub use order::{max, min, order_statistic, rank};
pub use summarize::{
    correlation, covariance, covariance_population, kurtosis, mean, median, quantile,
    quantile_sorted, skewness, std_dev, std_dev_population, sum, variance, variance_population,
};
pub use weighted::{
    weighted_correlation, weighted_covariance, weighted_mean, weighted_quantile, weighted_variance,
    weighted_variance_population,
};
