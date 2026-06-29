//! Chart trendlines (Phase 1.2). A trendline is an opt-in overlay
//! drawn over a chart's series — the standard "best-fit" line that
//! makes a trend visible at a glance. This module is pure (no DOM,
//! no web-sys) so the regression math is host-testable under
//! `cargo test`.
//!
//! Three kinds are supported:
//!
//! - [`Trendline::Linear`]: y = a + b*x. Ordinary least squares.
//! - [`Trendline::Exponential`]: y = a * exp(b*x). Log-linear least
//!   squares (take `ln(y)` first); `None` if any data point is `<= 0`.
//! - [`Trendline::Polynomial`]: y = a + b*x + c*x^2. Quadratic least
//!   squares solved via the normal equations.
//!
//! `x` is the series index `0..n`; `y` is the data point value. This
//! matches Excel's behaviour for index-based trendlines and keeps the
//! regression independent of the chart's pixel layout.

use serde::{Deserialize, Serialize};

/// Which trendline (if any) to draw on a chart. Persisted as a
/// lowercase string on disk via `#[serde(rename_all = "lowercase")]` so
/// older workbooks without the field default to `None` via the
/// `#[serde(default)]` on `Chart::trendline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Trendline {
    #[default]
    None,
    Linear,
    Exponential,
    Polynomial,
}

impl Trendline {
    /// True if this kind produces a visible line on the chart.
    pub fn is_visible(self) -> bool {
        !matches!(self, Trendline::None)
    }
}

/// Result of a linear regression `y = a + b*x`. `a` is the intercept,
/// `b` is the slope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearFit {
    pub intercept: f64,
    pub slope: f64,
}

/// Result of an exponential regression `y = a * exp(b*x)`. `a` and `b`
/// are the coefficients recovered from log-linear least squares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialFit {
    pub a: f64,
    pub b: f64,
}

/// Result of a quadratic regression `y = a + b*x + c*x^2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadraticFit {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

/// Linear least squares over the index-position `x = 0..n` and the
/// values `ys`. Returns `None` if `ys` has fewer than two points or
/// the slope is degenerate (no variation in x — only true if n == 1,
/// already guarded).
pub fn linear_regression(ys: &[f64]) -> Option<LinearFit> {
    let n = ys.len();
    if n < 2 {
        return None;
    }
    let n_f = n as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (i, &y) in ys.iter().enumerate() {
        let x = i as f64;
        sx += x;
        sy += y;
        sxy += x * y;
        sxx += x * x;
    }
    let mean_x = sx / n_f;
    let mean_y = sy / n_f;
    let denom = sxx - n_f * mean_x * mean_x;
    if denom == 0.0 {
        return None;
    }
    let slope = (sxy - n_f * mean_x * mean_y) / denom;
    let intercept = mean_y - slope * mean_x;
    Some(LinearFit { intercept, slope })
}

/// Exponential least squares `y = a * exp(b*x)` over `ys`. Returns
/// `None` if any value is `<= 0` (can't take `ln`), if `n < 2`, or if
/// x has no variance. `b` is in `ln-units`; the caller paints the
/// curve as `a * exp(b * x)` over the series index.
pub fn exponential_regression(ys: &[f64]) -> Option<ExponentialFit> {
    let n = ys.len();
    if n < 2 {
        return None;
    }
    let ln_ys: Vec<f64> = ys
        .iter()
        .map(|&y| if y > 0.0 { y.ln() } else { f64::NAN })
        .collect();
    if ln_ys.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let n_f = n as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (i, &ly) in ln_ys.iter().enumerate() {
        let x = i as f64;
        sx += x;
        sy += ly;
        sxy += x * ly;
        sxx += x * x;
    }
    let mean_x = sx / n_f;
    let mean_y = sy / n_f;
    let denom = sxx - n_f * mean_x * mean_x;
    if denom == 0.0 {
        return None;
    }
    let b = (sxy - n_f * mean_x * mean_y) / denom;
    let ln_a = mean_y - b * mean_x;
    Some(ExponentialFit { a: ln_a.exp(), b })
}

/// Quadratic least squares `y = a + b*x + c*x^2`. Solves the normal
/// equations directly (3×3 system). Returns `None` if `n < 3` (the
/// fit is underdetermined).
pub fn quadratic_regression(ys: &[f64]) -> Option<QuadraticFit> {
    let n = ys.len();
    if n < 3 {
        return None;
    }
    let n_f = n as f64;
    // Sum powers of x = 0..n.
    let mut sx = 0.0;
    let mut sx2 = 0.0;
    let mut sx3 = 0.0;
    let mut sx4 = 0.0;
    let mut sy = 0.0;
    let mut sxy = 0.0;
    let mut sx2y = 0.0;
    for (i, &y) in ys.iter().enumerate() {
        let x = i as f64;
        let x2 = x * x;
        sx += x;
        sx2 += x2;
        sx3 += x2 * x;
        sx4 += x2 * x2;
        sy += y;
        sxy += x * y;
        sx2y += x2 * y;
    }
    // Normal equations: [n, sx, sx2] [a]   [sy]
    //                  [sx, sx2, sx3][b] = [sxy]
    //                  [sx2, sx3, sx4][c]   [sx2y]
    // Solve via Cramer's rule — simpler than LU for 3×3.
    let det = |a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f64| {
        a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    };
    let m_det = det(n_f, sx, sx2, sx, sx2, sx3, sx2, sx3, sx4);
    if m_det.abs() < 1e-12 {
        return None;
    }
    let a_det = det(sy, sx, sx2, sxy, sx2, sx3, sx2y, sx3, sx4);
    let b_det = det(n_f, sy, sx2, sx, sxy, sx3, sx2, sx2y, sx4);
    let c_det = det(n_f, sx, sy, sx, sx2, sxy, sx2, sx3, sx2y);
    Some(QuadraticFit {
        a: a_det / m_det,
        b: b_det / m_det,
        c: c_det / m_det,
    })
}

/// Evaluate a linear fit at index `i`.
pub fn linear_eval(fit: LinearFit, i: usize) -> f64 {
    fit.intercept + fit.slope * (i as f64)
}

/// Evaluate an exponential fit at index `i`.
pub fn exponential_eval(fit: ExponentialFit, i: usize) -> f64 {
    fit.a * (fit.b * (i as f64)).exp()
}

/// Evaluate a quadratic fit at index `i`.
pub fn quadratic_eval(fit: QuadraticFit, i: usize) -> f64 {
    let x = i as f64;
    fit.a + fit.b * x + fit.c * x * x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_regression_recovers_known_line() {
        // y = 3 + 2x at indices 0..6 → [3, 5, 7, 9, 11, 13]
        let ys = [3.0, 5.0, 7.0, 9.0, 11.0, 13.0];
        let fit = linear_regression(&ys).unwrap();
        assert!((fit.intercept - 3.0).abs() < 1e-9);
        assert!((fit.slope - 2.0).abs() < 1e-9);
    }

    #[test]
    fn linear_regression_handles_scattered_data() {
        // Five points exactly on y = 1 + 0.5*x (no noise), recovered
        // with full precision.
        let ys = [1.0, 1.5, 2.0, 2.5, 3.0];
        let fit = linear_regression(&ys).unwrap();
        assert!((fit.intercept - 1.0).abs() < 1e-9);
        assert!((fit.slope - 0.5).abs() < 1e-9);
    }

    #[test]
    fn linear_regression_too_few_points_returns_none() {
        assert!(linear_regression(&[]).is_none());
        assert!(linear_regression(&[42.0]).is_none());
    }

    #[test]
    fn linear_eval_at_index() {
        let fit = LinearFit {
            intercept: 3.0,
            slope: 2.0,
        };
        assert_eq!(linear_eval(fit, 0), 3.0);
        assert_eq!(linear_eval(fit, 5), 13.0);
    }

    #[test]
    fn exponential_regression_recovers_known_curve() {
        // y = 2 * exp(0.5 * x) at indices 0..5.
        let ys: Vec<f64> = (0..5).map(|i| 2.0 * (0.5 * i as f64).exp()).collect();
        let fit = exponential_regression(&ys).unwrap();
        assert!((fit.a - 2.0).abs() < 1e-6);
        assert!((fit.b - 0.5).abs() < 1e-9);
    }

    #[test]
    fn exponential_regression_returns_none_for_nonpositive_data() {
        // log-linear regression is undefined for y <= 0.
        let ys = [1.0, 2.0, 0.0, 4.0];
        assert!(exponential_regression(&ys).is_none());
        let ys = [1.0, 2.0, -1.0, 4.0];
        assert!(exponential_regression(&ys).is_none());
    }

    #[test]
    fn exponential_regression_too_few_points_returns_none() {
        assert!(exponential_regression(&[]).is_none());
        assert!(exponential_regression(&[1.0]).is_none());
    }

    #[test]
    fn exponential_eval_at_index() {
        let fit = ExponentialFit { a: 2.0, b: 0.5 };
        assert!((exponential_eval(fit, 0) - 2.0).abs() < 1e-9);
        assert!((exponential_eval(fit, 2) - 2.0 * 1.0_f64.exp()).abs() < 1e-9);
    }

    #[test]
    fn quadratic_regression_recovers_known_parabola() {
        // y = 1 + 2x + 3x^2 at indices 0..6.
        let ys: Vec<f64> = (0..6)
            .map(|i| 1.0 + 2.0 * i as f64 + 3.0 * (i as f64).powi(2))
            .collect();
        let fit = quadratic_regression(&ys).unwrap();
        assert!((fit.a - 1.0).abs() < 1e-6);
        assert!((fit.b - 2.0).abs() < 1e-6);
        assert!((fit.c - 3.0).abs() < 1e-6);
    }

    #[test]
    fn quadratic_regression_too_few_points_returns_none() {
        assert!(quadratic_regression(&[]).is_none());
        assert!(quadratic_regression(&[1.0]).is_none());
        assert!(quadratic_regression(&[1.0, 2.0]).is_none());
    }

    #[test]
    fn quadratic_eval_at_index() {
        let fit = QuadraticFit {
            a: 1.0,
            b: 2.0,
            c: 3.0,
        };
        assert_eq!(quadratic_eval(fit, 0), 1.0);
        assert_eq!(quadratic_eval(fit, 1), 6.0);
        assert_eq!(quadratic_eval(fit, 2), 17.0);
    }

    #[test]
    fn trendline_default_is_none() {
        assert_eq!(Trendline::default(), Trendline::None);
    }

    #[test]
    fn trendline_is_visible_only_for_nonzero_kinds() {
        assert!(!Trendline::None.is_visible());
        assert!(Trendline::Linear.is_visible());
        assert!(Trendline::Exponential.is_visible());
        assert!(Trendline::Polynomial.is_visible());
    }

    #[test]
    fn trendline_serde_round_trips() {
        for k in [
            Trendline::None,
            Trendline::Linear,
            Trendline::Exponential,
            Trendline::Polynomial,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: Trendline = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back);
        }
    }

    #[test]
    fn trendline_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Trendline::Linear).unwrap(),
            "\"linear\""
        );
        assert_eq!(
            serde_json::to_string(&Trendline::Exponential).unwrap(),
            "\"exponential\""
        );
        assert_eq!(
            serde_json::to_string(&Trendline::Polynomial).unwrap(),
            "\"polynomial\""
        );
        assert_eq!(serde_json::to_string(&Trendline::None).unwrap(), "\"none\"");
    }
}
