//! The delta method — propagation of variance through a nonlinear transform.
//!
//! Given an estimator `X` with variance `Var(X)` and a differentiable function
//! `g`, the first-order approximation is
//!
//! ```text
//! Var(g(X)) ≈ Var(X) · [g'(X)]².
//! ```
//!
//! [`delta_method`] evaluates `g'` numerically with a central finite
//! difference. This keeps the API dependency-free and works for any `g` you can
//! evaluate; the cost is one extra function evaluation per call and the usual
//! finite-difference caveats near discontinuities. Where an analytic derivative
//! is available, prefer it.

/// Propagate variance through `g` via the delta method.
///
/// `var_x` is `Var(X)`; the returned value is the approximate `Var(g(X))`.
///
/// The step size is `h = max(1, |x|) · ∛ε`, which balances truncation and
/// round-off error for the central-difference formula.
pub fn delta_method(g: impl Fn(f64) -> f64, x: f64, var_x: f64) -> f64 {
    let h = x.abs().max(1.0) * f64::EPSILON.cbrt();
    let deriv = (g(x + h) - g(x - h)) / (2.0 * h);
    var_x * deriv * deriv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn linear_transform_scales_variance_by_slope_squared() {
        // g(x) = 2x + 3 → g' = 2 → Var(g) = 4·Var(x).
        let g = |x: f64| 2.0 * x + 3.0;
        assert!(approx_eq(delta_method(g, 10.0, 5.0), 20.0, 1e-9));
    }

    #[test]
    fn square_at_known_point() {
        // g(x) = x², evaluated at x=3 with Var=2: g'=6 → Var ≈ 36·2 = 72.
        let g = |x: f64| x * x;
        assert!(approx_eq(delta_method(g, 3.0, 2.0), 72.0, 1e-6));
    }

    #[test]
    fn exponential_transform() {
        // g(x) = e^x at x=0 with Var=1: g'=1 → Var ≈ 1.
        let g = |x: f64| x.exp();
        assert!(approx_eq(delta_method(g, 0.0, 1.0), 1.0, 1e-6));
    }
}
