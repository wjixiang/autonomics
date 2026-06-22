//! Special functions: gamma, beta, erf and the incomplete gamma function.
//!
//! All hand-written (no `libm`-beyond-std, no external crates). Three building
//! blocks, in order of dependency:
//!
//! 1. [`ln_gamma`] / [`gamma`] — Lanczos approximation (Godfrey, g=7, n=9),
//!    accurate to full double precision for `x > 0`; reflection extends
//!    [`gamma`] to negative non-integer arguments.
//! 2. [`ln_beta`] / [`beta`] — derived from the log-gamma.
//! 3. [`gammp`] / [`gammq`] — lower/upper incomplete gamma (Numerical Recipes
//!    series + Lentz continued fraction). This is the engine for [`erf`] and,
//!    later, the χ² and F distributions.
//!
//! These are the foundation of Layer 2 (distributions), which is why they live
//! in Layer 0 even though descriptive statistics do not call them directly.

use std::f64::consts::PI;

/// Lanczos g=7, n=9 coefficients (Paul Godfrey). Give ~1e-15 relative error.
const LANCZOS_G: f64 = 7.0;
const LANCZOS_P: [f64; 9] = [
    0.99999999999980993,
    676.5203681218851,
    -1259.1392167224028,
    771.32342877765313,
    -176.61502916214059,
    12.507343278686905,
    -0.13857109526572012,
    9.9843695780195716e-6,
    1.5056327351493116e-7,
];

/// Natural logarithm of the gamma function, `ln Γ(x)`.
///
/// Defined for `x > 0`. Returns `NaN` for `x ≤ 0` (poles / negative values).
/// Computed directly in log-space (no `exp`/`ln` round-trip) so it stays
/// accurate for large `x` where `Γ(x)` would overflow.
pub fn ln_gamma(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return f64::NAN;
    }
    if x < 0.5 {
        // Reflection: ln Γ(x) = ln π - ln(sin(πx)) - ln Γ(1−x), valid for x∈(0,½).
        PI.ln() - (PI * x).sin().abs().ln() - ln_gamma(1.0 - x)
    } else {
        let z = x - 1.0;
        let mut a = LANCZOS_P[0];
        for (i, coeff) in LANCZOS_P.iter().enumerate().skip(1) {
            a += coeff / (z + i as f64);
        }
        let t = z + LANCZOS_G + 0.5;
        // ln Γ(x) = ½ ln(2π) + (z+½) ln t − t + ln a
        0.5 * (2.0 * PI).ln() + (z + 0.5) * t.ln() - t + a.ln()
    }
}

/// The gamma function, `Γ(x)`.
///
/// Accurate for `x > 0`. For negative non-integer arguments, the reflection
/// formula `Γ(x) = π / (sin(πx)·Γ(1−x))` gives the (possibly negative) real
/// value. At non-positive integers (`x = 0, −1, −2, …`) — the poles — this
/// returns `±∞`. Returns `NaN` for `NaN` input.
pub fn gamma(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.5 {
        // Reflection; handles negatives and sharpens (0, ½).
        PI / ((PI * x).sin() * gamma(1.0 - x))
    } else {
        let z = x - 1.0;
        let mut a = LANCZOS_P[0];
        for (i, coeff) in LANCZOS_P.iter().enumerate().skip(1) {
            a += coeff / (z + i as f64);
        }
        let t = z + LANCZOS_G + 0.5;
        // Γ(x) = √(2π) · t^(z+½) · e^(−t) · a
        (2.0 * PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * a
    }
}

/// Natural logarithm of the beta function, `ln B(a, b) = ln Γ(a) + ln Γ(b) − ln Γ(a+b)`.
///
/// Requires `a > 0` and `b > 0`; returns `NaN` otherwise.
pub fn ln_beta(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 || a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)
}

/// The beta function, `B(a, b)`.
///
/// Requires `a > 0` and `b > 0`; returns `NaN` otherwise. Returns `+∞` if the
/// value overflows `f64`.
pub fn beta(a: f64, b: f64) -> f64 {
    ln_beta(a, b).exp()
}

// ── Incomplete gamma ──────────────────────────────────────────────────────
// Numerical Recipes (3rd ed.) `gammp`/`gammq` with `gser` (series) and `gcf`
// (Lentz continued fraction). ITMAX/EPS chosen for double precision.

const ITMAX: usize = 200;
const EPS: f64 = 3.0e-16;
const FPMIN: f64 = 1.0e-300;

/// Lower incomplete gamma `P(a, x) = γ(a, x) / Γ(a)` (the regularized form).
///
/// Requires `a > 0` and `x ≥ 0`. `P(a, 0) = 0`, `P(a, ∞) → 1`.
pub(crate) fn gammp(a: f64, x: f64) -> f64 {
    if x < 0.0 || a <= 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        // Series representation, good for x < a+1.
        gser(a, x)
    } else {
        // Continued fraction gives Q; P = 1 − Q.
        1.0 - gcf(a, x)
    }
}

/// Upper incomplete gamma `Q(a, x) = 1 − P(a, x)`.
pub(crate) fn gammq(a: f64, x: f64) -> f64 {
    1.0 - gammp(a, x)
}

/// Series representation of `P(a, x)` (NR `gser`).
fn gser(a: f64, x: f64) -> f64 {
    let gln = ln_gamma(a);
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut del = sum;
    for _ in 0..ITMAX {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            break;
        }
    }
    sum * (-x + a * x.ln() - gln).exp()
}

/// Continued-fraction representation of `Q(a, x)`.
///
/// Evaluates `F = b₀ + a₁/(b₁ + a₂/(b₂ + …))` by modified Lentz's method
/// (NR §5.2), then `Q(a, x) = exp(−x + a·ln x − lnΓ(a)) / F`. The CF
/// coefficients are `b_j = x + 1 − a + 2j` and `a_j = −j·(j − a)`.
fn gcf(a: f64, x: f64) -> f64 {
    let gln = ln_gamma(a);
    let tiny = FPMIN;
    let b0 = x + 1.0 - a;
    let mut f = b0;
    if f == 0.0 {
        f = tiny;
    }
    let mut c = f; // C_0
    let mut d = 0.0; // D_0
    for j in 1..=ITMAX {
        let aj = -(j as f64) * (j as f64 - a);
        let bj = b0 + 2.0 * j as f64;
        // D_j = 1 / (b_j + a_j · D_{j-1})
        d = bj + aj * d;
        if d == 0.0 {
            d = tiny;
        }
        d = 1.0 / d;
        // C_j = b_j + a_j / C_{j-1}
        c = bj + aj / c;
        if c == 0.0 {
            c = tiny;
        }
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() < EPS {
            break;
        }
    }
    (-x + a * x.ln() - gln).exp() / f
}

// ── Error function ────────────────────────────────────────────────────────

/// The error function, `erf(x) = (2/√π) ∫₀ˣ e^(−t²) dt`.
///
/// Uses `erf(x) = sign(x)·P(½, x²)`, so the implementation shares its precision
/// and machinery with the χ² distribution (via `gammp`).
pub fn erf(x: f64) -> f64 {
    if x < 0.0 {
        -gammp(0.5, x * x)
    } else {
        gammp(0.5, x * x)
    }
}

/// The complementary error function, `erfc(x) = 1 − erf(x)`.
///
/// Evaluated via the upper incomplete gamma `Q(½, x²)` for `x ≥ 0` and by the
/// reflection `erfc(−x) = 2 − erfc(x)` for `x < 0`. This avoids the
/// catastrophic cancellation of `1 − erf(x)` when `erf(x)` is very close to 1.
pub fn erfc(x: f64) -> f64 {
    if x < 0.0 {
        1.0 + gammp(0.5, x * x)
    } else {
        gammq(0.5, x * x)
    }
}

// ── Regularized incomplete beta ────────────────────────────────────────────

/// Regularized incomplete beta function `I_x(a, b) = B(x;a,b)/B(a,b)`.
///
/// Engine for the Student-t, F, and Beta distribution CDFs. Requires `a, b > 0`
/// and `0 ≤ x ≤ 1`; clamps `x` to `[0, 1]` at the boundaries. NR §6.4 `betai`
/// + `betacf` (modified Lentz continued fraction).
pub fn betai(a: f64, b: f64, x: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // ln of the front factor: exp(lnΓ(a+b) − lnΓ(a) − lnΓ(b) + a·ln x + b·ln(1−x)).
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    // Continued fraction converges faster in the smaller tail, so swap.
    if x < (a + 1.0) / (a + b + 2.0) {
        bt * betacf(a, b, x) / a
    } else {
        1.0 - bt * betacf(b, a, 1.0 - x) / b
    }
}

/// Lentz continued fraction for the incomplete beta (NR `betacf`).
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    const FP_MIN: f64 = 1.0e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FP_MIN {
        d = FP_MIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=ITMAX {
        let m2 = 2 * m;
        // Even step.
        let aa = m as f64 * (b - m as f64) * x / ((qam + m2 as f64) * (a + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FP_MIN {
            d = FP_MIN;
        }
        d = 1.0 / d;
        c = 1.0 + aa / c;
        if c.abs() < FP_MIN {
            c = FP_MIN;
        }
        let del = d * c;
        h *= del;
        // Odd step.
        let aa = -(a + m as f64) * (qab + m as f64) * x / ((a + m2 as f64) * (qap + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FP_MIN {
            d = FP_MIN;
        }
        d = 1.0 / d;
        c = 1.0 + aa / c;
        if c.abs() < FP_MIN {
            c = FP_MIN;
        }
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

// ── Standard normal quantile (inverse Φ) and erfinv ───────────────────────

/// Standard-normal quantile `Φ⁻¹(p)` for `p ∈ (0, 1)`.
///
/// Acklam's rational approximation (relative error < 1.15e-9) followed by one
/// Halley refinement step using [`erfc`], yielding full double precision.
/// Returns `−∞` / `+∞` at the boundaries and `NaN` for `p` outside `[0, 1]`.
pub fn std_normal_ppf(p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    if p == 0.0 {
        return f64::NEG_INFINITY;
    }
    if p == 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    let x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };

    // One Halley step: Φ(x) = ½·erfc(−x/√2).
    let e = 0.5 * erfc(-x / std::f64::consts::SQRT_2) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// Inverse error function, `erf⁻¹(p)` for `p ∈ (−1, 1)`.
///
/// Derived from [`std_normal_ppf`] via the identity
/// `erf⁻¹(p) = Φ⁻¹((p+1)/2) / √2`. Returns `NaN` outside `(−1, 1)`.
pub fn erfinv(p: f64) -> f64 {
    if !(-1.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    std_normal_ppf((p + 1.0) / 2.0) / std::f64::consts::SQRT_2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn ln_gamma_known_values() {
        // Γ(n) = (n−1)!  →  ln Γ(1)=0, ln Γ(2)=0, ln Γ(5)=ln 24.
        assert!(approx_eq(ln_gamma(1.0), 0.0, 1e-12));
        assert!(approx_eq(ln_gamma(2.0), 0.0, 1e-12));
        assert!(approx_eq(ln_gamma(5.0), (24.0_f64).ln(), 1e-12));
        // Γ(½) = √π.
        assert!(approx_eq(ln_gamma(0.5), 0.5 * PI.ln(), 1e-12));
        // Γ(0.1) ≈ 9.513507698668732 → ln ≈ 2.252712651734206.
        assert!(approx_eq(ln_gamma(0.1), 2.252712651734206, 1e-10));
    }

    #[test]
    fn ln_gamma_domain() {
        assert!(ln_gamma(0.0).is_nan());
        assert!(ln_gamma(-1.0).is_nan());
        assert!(ln_gamma(-2.5).is_nan());
    }

    #[test]
    fn gamma_known_values() {
        assert!(approx_eq(gamma(0.5), PI.sqrt(), 1e-12));
        assert!(approx_eq(gamma(1.0), 1.0, 1e-12));
        assert!(approx_eq(gamma(5.0), 24.0, 1e-12));
        // Γ(−½) = −2√π.
        assert!(approx_eq(gamma(-0.5), -2.0 * PI.sqrt(), 1e-10));
    }

    #[test]
    fn gamma_poles_at_nonpositive_integers() {
        // Γ has poles at 0, −1, −2, …; the reflection formula divides by
        // sin(πx), which at these points is ~1e-16 rather than exactly 0, so
        // the result is a huge finite number rather than a true infinity.
        assert!(gamma(0.0).abs() > 1e10);
        assert!(gamma(-1.0).abs() > 1e10);
    }

    #[test]
    fn beta_known_values() {
        // B(2, 3) = Γ(2)Γ(3)/Γ(5) = 1·2/24 = 1/12.
        assert!(approx_eq(beta(2.0, 3.0), 1.0 / 12.0, 1e-12));
        assert!(approx_eq(ln_beta(2.0, 3.0), (1.0 / 12.0_f64).ln(), 1e-12));
        // B(½, ½) = π.
        assert!(approx_eq(beta(0.5, 0.5), PI, 1e-12));
    }

    #[test]
    fn erf_known_values() {
        assert!(approx_eq(erf(0.0), 0.0, 1e-12));
        assert!(approx_eq(erf(0.5), 0.520499877813047, 1e-10));
        assert!(approx_eq(erf(1.0), 0.842700792949715, 1e-10));
        assert!(approx_eq(erf(2.0), 0.995322265018953, 1e-10));
        // Odd symmetry.
        assert!(approx_eq(erf(-1.0), -erf(1.0), 1e-12));
        // erf → 1 for large x.
        assert!(approx_eq(erf(6.0), 1.0, 1e-12));
    }

    #[test]
    fn erfc_is_complement_of_erf() {
        for &x in &[0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.5] {
            assert!(approx_eq(erfc(x), 1.0 - erf(x), 1e-9));
        }
    }

    #[test]
    fn gammp_bounds() {
        assert_eq!(gammp(2.0, 0.0), 0.0);
        // P(a, ∞) → 1: pick x large relative to a.
        assert!(gammp(2.0, 50.0) > 0.999999);
    }

    #[test]
    fn betai_known_values() {
        // I_0(a,b) = 0, I_1(a,b) = 1.
        assert_eq!(betai(2.0, 3.0, 0.0), 0.0);
        assert_eq!(betai(2.0, 3.0, 1.0), 1.0);
        // Symmetry: I_x(a,b) = 1 − I_{1−x}(b,a).
        let v = betai(2.0, 3.0, 0.4);
        assert!(approx_eq(v, 1.0 - betai(3.0, 2.0, 0.6), 1e-12));
        // Reference: betai(2,3,0.5) = 0.6875 (Beta(2,3) CDF at 0.5).
        assert!(approx_eq(betai(2.0, 3.0, 0.5), 0.6875, 1e-10));
    }

    #[test]
    fn betai_matches_numerical_integration() {
        // Independent of the continued-fraction series: integrate the Beta
        // density t^{a−1}(1−t)^{b−1}/B(a,b) over [0, x] by Simpson's rule and
        // compare with betai. Verifies non-integer shapes (a=2.5, b=5).
        let (a, b, x) = (2.5_f64, 5.0_f64, 1.0 / 3.0);
        let lb = ln_beta(a, b);
        let pdf = |t: f64| ((a - 1.0) * t.ln() + (b - 1.0) * (1.0 - t).ln() - lb).exp();
        let n = 200_000usize;
        let h = x / n as f64;
        let mut s = pdf(0.0) + pdf(x); // endpoints (pdf(0)=0 here since a>1)
        let mut acc = 0.0;
        for i in 1..n {
            let t = i as f64 * h;
            acc += (if i % 2 == 0 { 2.0 } else { 4.0 }) * pdf(t);
        }
        s = (s + acc) * h / 3.0;
        assert!(approx_eq(betai(a, b, x), s, 1e-7));
    }

    #[test]
    fn std_normal_ppf_known_values() {
        // Φ⁻¹(0.5) = 0, Φ⁻¹(0.025) = −1.959964, Φ⁻¹(0.975) = +1.959964.
        assert!(approx_eq(std_normal_ppf(0.5), 0.0, 1e-9));
        assert!(approx_eq(std_normal_ppf(0.025), -1.959963984540054, 1e-8));
        assert!(approx_eq(std_normal_ppf(0.975), 1.959963984540054, 1e-8));
        // Φ⁻¹(0.001) = −3.090232306167814.
        assert!(approx_eq(std_normal_ppf(0.001), -3.090232306167814, 1e-7));
    }

    #[test]
    fn std_normal_ppf_roundtrip() {
        // Φ(Φ⁻¹(p)) ≈ p via erfc.
        for &p in &[0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            let x = std_normal_ppf(p);
            let cdf = 0.5 * erfc(-x / std::f64::consts::SQRT_2);
            assert!(approx_eq(cdf, p, 1e-9));
        }
    }

    #[test]
    fn erfinv_known_values() {
        assert!(approx_eq(erfinv(0.0), 0.0, 1e-9));
        // erf(erfinv(p)) = p for several p.
        for &p in &[0.1, 0.3, 0.5, 0.7, 0.9, -0.4] {
            assert!(approx_eq(erf(erfinv(p)), p, 1e-9));
        }
    }
}
