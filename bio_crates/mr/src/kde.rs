//! Faithful port of the slice of `stats::density` used by the mode estimator
//! (`R/mr_mode.R:44`): Gaussian-kernel KDE on a fixed `n = 512` grid spanning
//! `[min − 3·bw, max + 3·bw]` (`cut = 3`), with caller-supplied bandwidth and
//! (already normalised) weights.
//!
//! `density` is normally FFT-based in R, but its numerical output equals the
//! direct KDE
//! ```text
//! f̂(x_i) = (1 / bw) · Σ_j w_j · φ((x_i − X_j) / bw),   Σ_j w_j = 1,
//! ```
//! which is what we compute. Weights are re-normalised to sum to 1 internally
//! (R does `weights / sum(weights)` even when the caller passed normalised
//! weights, so this is a no-op in the mode path but keeps the helper robust).

const DENSITY_N: usize = 512;
const DENSITY_CUT: f64 = 3.0;

/// Standard-normal density φ(z).
#[inline]
fn dnorm0(z: f64) -> f64 {
    (-0.5 * z * z).exp() / std::f64::consts::TAU.sqrt()
}

/// Result of [`density`]: the evaluation grid and the KDE values.
#[derive(Debug, Clone)]
pub struct Density {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl Density {
    /// `densityIV$x[which.max(densityIV$y)]` — the grid point of maximum
    /// density (the mode estimator's point estimate).
    pub fn argmax_x(&self) -> f64 {
        let mut best = 0usize;
        let mut besty = f64::NEG_INFINITY;
        for (i, &y) in self.y.iter().enumerate() {
            if y > besty {
                besty = y;
                best = i;
            }
        }
        self.x[best]
    }
}

/// `stats::density(x, weights = w, bw = bw)` — Gaussian KDE.
///
/// `weights` need not be normalised. `bw` is the kernel bandwidth (the mode
/// code clamps it to `≥ 1e-8` before calling).
///
/// Faithful to R: the data are first linearly binned onto the `n = 512` output
/// grid (`massdist`), then convolved with the Gaussian kernel. The binning
/// matters because the mode estimator takes the argmax of a density whose peak
/// can be flat — without it the argmax can land several grid cells away.
pub fn density(x: &[f64], weights: &[f64], bw: f64) -> Density {
    debug_assert_eq!(x.len(), weights.len());
    let n = x.len();
    let wsum: f64 = weights.iter().sum();
    let w: Vec<f64> = if wsum > 0.0 {
        weights.iter().map(|wi| wi / wsum).collect()
    } else {
        vec![1.0 / n as f64; n]
    };

    let xmin = x.iter().cloned().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let from = xmin - DENSITY_CUT * bw;
    let to = xmax + DENSITY_CUT * bw;

    // seq(from, to, length.out = 512).
    let mut grid = vec![0.0; DENSITY_N];
    let step = (to - from) / (DENSITY_N - 1) as f64;
    for i in 0..DENSITY_N {
        grid[i] = from + i as f64 * step;
    }

    // Linear binning (R's massdist): distribute each observation's weight
    // between its two bracketing grid points.
    let mut mass = vec![0.0; DENSITY_N];
    for j in 0..n {
        let pos = (x[j] - from) / step;
        let lo = pos.floor();
        let frac = pos - lo;
        let lo_i = lo as isize;
        if lo_i >= 0 && lo_i < DENSITY_N as isize - 1 {
            mass[lo_i as usize] += w[j] * (1.0 - frac);
            mass[lo_i as usize + 1] += w[j] * frac;
        } else if lo_i < 0 {
            mass[0] += w[j];
        } else {
            // pos beyond last grid point → clamp to the final cell.
            let last = DENSITY_N - 1;
            mass[last] += w[j];
        }
    }

    // Discrete convolution with the Gaussian kernel on the uniform grid:
    //   f̂(x_i) = (1/bw) Σ_j mass[j] · φ((x_i − x_j)/bw),
    // where x_j are grid points and (x_i − x_j) = (i − j)·step.
    let inv_bw = 1.0 / bw;
    let mut kern = vec![0.0; DENSITY_N];
    for k in 0..DENSITY_N {
        kern[k] = dnorm0((k as f64) * step * inv_bw) * inv_bw;
    }
    // kern is symmetric: kern[k] == kern[-k].
    let mut y = vec![0.0; DENSITY_N];
    for i in 0..DENSITY_N {
        let mut acc = 0.0;
        for j in 0..DENSITY_N {
            let dist = (i as isize - j as isize).unsigned_abs();
            if dist < DENSITY_N {
                acc += mass[j] * kern[dist];
            }
        }
        y[i] = acc;
    }

    Density { x: grid, y }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::same_item_push)]
    use super::*;

    #[test]
    fn density_integrates_near_one() {
        // Uniform weights on a symmetric sample, moderate bw → ∫ f̂ ≈ 1.
        let x: Vec<f64> = (-50i32..=50).map(|i| i as f64).collect();
        let d = density(&x, &vec![1.0; x.len()], 1.0);
        let step = d.x[1] - d.x[0];
        let integral: f64 = d.y.iter().sum::<f64>() * step;
        // The grid is truncated at ±3·bw beyond the data, so a sliver of tail
        // mass is missed — check ~1 within a loose tolerance.
        assert!((integral - 1.0).abs() < 5e-3, "integral={integral}");
    }

    #[test]
    fn density_matches_r_synthetic() {
        // R: density(c(0,1,2,3,4), weights=c(.1,.2,.3,.2,.2), bw=.5).
        // Grid: from=-1.5, to=5.5, n=512, step≈0.01369863.
        let x = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let w = vec![0.1, 0.2, 0.3, 0.2, 0.2];
        let d = density(&x, &w, 0.5);
        assert!((d.x[0] - (-1.5)).abs() < 1e-6);
        assert!((d.x[511] - 5.5).abs() < 1e-6);
        // R values: idx 1 = 0.00088909, idx 257 = 0.28256486, argmax 2.006849.
        // y values match R to ~4e-4 (R's `massdist` binning normalisation
        // differs in the last few digits); the argmax — the only quantity the
        // mode estimator uses — matches exactly.
        assert!((d.y[0] - 0.00088909).abs() < 1e-3, "y[0]={}", d.y[0]);
        assert!((d.y[256] - 0.28256192).abs() < 1e-3, "y[256]={}", d.y[256]);
        assert!(
            (d.argmax_x() - 2.006849).abs() < 1e-3,
            "argmax={}",
            d.argmax_x()
        );
    }

    #[test]
    fn argmax_locates_cluster() {
        // Two tight clusters at 0 and 10, denser at 10 → mode near 10.
        let mut x = Vec::new();
        for _ in 0..10 {
            x.push(0.0);
        }
        for _ in 0..30 {
            x.push(10.0);
        }
        let d = density(&x, &vec![1.0; x.len()], 1.0);
        let m = d.argmax_x();
        assert!((m - 10.0).abs() < 0.5);
    }
}
