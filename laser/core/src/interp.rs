// SPDX-License-Identifier: GPL-3.0-or-later
//! Interpolation through time for baselines and reference-material
//! normalisation. Weights are inverse variances: the spline minimises
//! sum(w_i * (y_i - f(x_i))^2) + lambda * integral(f''(x)^2).
//! "Auto" chooses lambda by 8-fold cross-validation, then takes the smoothest
//! curve within 2 % of the best score. It is not the same method as other
//! programs' auto-smooth, so curves can differ slightly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InterpKind {
    Mean,
    /// Weighted straight-line fit through all points.
    LinearFit,
    /// Straight lines joining neighbouring points.
    Connect,
    StepForward,
    SmoothAuto,
    /// Smoothing spline with a fixed log10(lambda).
    SmoothFixed(f64),
}

impl InterpKind {
    pub const CHOICES: [InterpKind; 6] = [
        InterpKind::Mean,
        InterpKind::LinearFit,
        InterpKind::Connect,
        InterpKind::StepForward,
        InterpKind::SmoothAuto,
        InterpKind::SmoothFixed(-3.0),
    ];
    pub fn label(&self) -> String {
        match self {
            InterpKind::Mean => "Mean".into(),
            InterpKind::LinearFit => "Linear fit".into(),
            InterpKind::Connect => "Join the dots".into(),
            InterpKind::StepForward => "Step forward".into(),
            InterpKind::SmoothAuto => "Smoothing spline (auto)".into(),
            InterpKind::SmoothFixed(_) => "Smoothing spline (fixed)".into(),
        }
    }
}

#[derive(Clone, Debug)]
enum Model {
    Const(f64),
    Line { a: f64, b: f64 },
    Step,
    Connect,
    Spline(Spline),
}

/// A fitted curve through (time, value) points.
#[derive(Clone, Debug)]
pub struct Curve {
    pub kind: InterpKind,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    /// 1 sigma uncertainty of each point.
    pub sigmas: Vec<f64>,
    /// Chosen log10(lambda) for spline fits.
    pub log_lambda: Option<f64>,
    model: Model,
}

impl Curve {
    pub fn eval(&self, x: f64) -> f64 {
        match &self.model {
            Model::Const(v) => *v,
            Model::Line { a, b } => a + b * x,
            Model::Step => {
                let i = self.xs.partition_point(|p| *p <= x);
                if i == 0 {
                    self.ys[0]
                } else {
                    self.ys[i - 1]
                }
            }
            Model::Connect => {
                let n = self.xs.len();
                let i = self.xs.partition_point(|p| *p <= x);
                if i == 0 {
                    self.ys[0]
                } else if i >= n {
                    self.ys[n - 1]
                } else {
                    let t = (x - self.xs[i - 1]) / (self.xs[i] - self.xs[i - 1]);
                    self.ys[i - 1] * (1.0 - t) + self.ys[i] * t
                }
            }
            Model::Spline(s) => s.eval(x),
        }
    }
}

/// Fit `kind` through finite points with 1 sigma uncertainties. Points are
/// sorted by x; returns None without any usable point.
pub fn fit(kind: InterpKind, xs: &[f64], ys: &[f64], sigmas: &[f64]) -> Option<Curve> {
    let mut pts: Vec<(f64, f64, f64)> = (0..xs.len())
        .filter(|&i| xs[i].is_finite() && ys[i].is_finite())
        .map(|i| {
            let s = if sigmas[i].is_finite() && sigmas[i] > 0.0 {
                sigmas[i]
            } else {
                f64::NAN
            };
            (xs[i], ys[i], s)
        })
        .collect();
    if pts.is_empty() {
        return None;
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    // A point without a usable uncertainty gets the median one, so one bad
    // estimate cannot pull the curve through itself.
    let known: Vec<f64> = pts.iter().map(|p| p.2).filter(|s| s.is_finite()).collect();
    let typical = if known.is_empty() { 1.0 } else { crate::stats::median(&known) };
    let x: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let y: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let s: Vec<f64> = pts
        .iter()
        .map(|p| if p.2.is_finite() { p.2 } else { typical })
        .collect();
    let w: Vec<f64> = s.iter().map(|v| 1.0 / (v * v)).collect();
    let n = x.len();
    let mut log_lambda = None;
    let model = match kind {
        InterpKind::Mean => {
            let sw: f64 = w.iter().sum();
            Model::Const(w.iter().zip(&y).map(|(w, y)| w * y).sum::<f64>() / sw)
        }
        InterpKind::LinearFit => {
            if n < 2 {
                Model::Const(y[0])
            } else {
                let (a, b) = line_fit(&x, &y, &w);
                Model::Line { a, b }
            }
        }
        InterpKind::Connect => Model::Connect,
        InterpKind::StepForward => Model::Step,
        InterpKind::SmoothAuto | InterpKind::SmoothFixed(_) => {
            if n == 1 {
                Model::Const(y[0])
            } else if n == 2 {
                let (a, b) = line_fit(&x, &y, &[1.0, 1.0]);
                Model::Line { a, b }
            } else {
                let ll = match kind {
                    InterpKind::SmoothFixed(l) => l,
                    _ => auto_log_lambda(&x, &y, &w),
                };
                log_lambda = Some(ll);
                Model::Spline(Spline::fit(&x, &y, &w, 10f64.powf(ll)))
            }
        }
    };
    Some(Curve {
        kind,
        xs: x,
        ys: y,
        sigmas: s,
        log_lambda,
        model,
    })
}

/// Jack-knifed relative residuals: each analysis removed in turn, the curve
/// refitted, the analysis compared with the refit. The first and last
/// analyses are left out because the refit would extrapolate. Returns
/// (relative residual, relative 1 sigma).
pub fn jackknife_residuals(kind: InterpKind, xs: &[f64], ys: &[f64], sigmas: &[f64]) -> Vec<(f64, f64)> {
    let n = xs.len();
    let mut out = Vec::new();
    if n < 4 {
        return out;
    }
    // Refit with the chosen lambda rather than re-tuning for each removal,
    // which would mix tuning noise into the residuals.
    let fixed = match kind {
        InterpKind::SmoothAuto => match fit(kind, xs, ys, sigmas) {
            Some(c) => c.log_lambda.map_or(kind, InterpKind::SmoothFixed),
            None => kind,
        },
        k => k,
    };
    for i in 1..n - 1 {
        let mut x = xs.to_vec();
        let mut y = ys.to_vec();
        let mut s = sigmas.to_vec();
        x.remove(i);
        y.remove(i);
        s.remove(i);
        if let Some(c) = fit(fixed, &x, &y, &s) {
            let f = c.eval(xs[i]);
            if f != 0.0 && f.is_finite() {
                out.push(((ys[i] - f) / f, sigmas[i] / f.abs()));
            }
        }
    }
    out
}

fn line_fit(x: &[f64], y: &[f64], w: &[f64]) -> (f64, f64) {
    let (mut sw, mut sx, mut sy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        sw += w[i];
        sx += w[i] * x[i];
        sy += w[i] * y[i];
    }
    let (mx, my) = (sx / sw, sy / sw);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for i in 0..x.len() {
        sxx += w[i] * (x[i] - mx).powi(2);
        sxy += w[i] * (x[i] - mx) * (y[i] - my);
    }
    if sxx <= 0.0 {
        return (my, 0.0);
    }
    let b = sxy / sxx;
    (my - b * mx, b)
}

fn auto_log_lambda(x: &[f64], y: &[f64], w: &[f64]) -> f64 {
    let n = x.len();
    let k = n.min(8);
    let grid: Vec<f64> = (0..=32).map(|i| -10.0 + i as f64 * 0.5).collect();
    let mut scores = Vec::with_capacity(grid.len());
    for ll in &grid {
        let lambda = 10f64.powf(*ll);
        let mut score = 0.0;
        for fold in 0..k {
            let (mut tx, mut ty, mut tw) = (Vec::new(), Vec::new(), Vec::new());
            for i in 0..n {
                if i % k != fold {
                    tx.push(x[i]);
                    ty.push(y[i]);
                    tw.push(w[i]);
                }
            }
            if tx.len() < 3 {
                continue;
            }
            let s = Spline::fit(&tx, &ty, &tw, lambda);
            for i in (fold..n).step_by(k) {
                score += w[i] * (y[i] - s.eval(x[i])).powi(2);
            }
        }
        scores.push(score);
    }
    let best = scores.iter().copied().fold(f64::INFINITY, f64::min);
    let mut chosen = grid[0];
    for (ll, s) in grid.iter().zip(&scores) {
        if *s <= best * 1.02 {
            chosen = *ll;
        }
    }
    chosen
}

/// Natural cubic smoothing spline (Reinsch), with x scaled to 0..1 and weights
/// scaled to mean 1, so lambda does not depend on units.
#[derive(Clone, Debug)]
pub struct Spline {
    x0: f64,
    span: f64,
    x: Vec<f64>,
    g: Vec<f64>,
    gamma: Vec<f64>,
}

impl Spline {
    pub fn fit(xs: &[f64], ys: &[f64], ws: &[f64], lambda: f64) -> Spline {
        let n = xs.len();
        let x0 = xs[0];
        let span = (xs[n - 1] - xs[0]).max(1e-12);
        let x: Vec<f64> = xs.iter().map(|v| (v - x0) / span).collect();
        let mean_w = ws.iter().sum::<f64>() / n as f64;
        let w: Vec<f64> = ws.iter().map(|v| v / mean_w).collect();
        let h: Vec<f64> = (0..n - 1).map(|i| (x[i + 1] - x[i]).max(1e-12)).collect();
        let m = n - 2;
        let mut a = vec![0.0; m];
        let mut b = vec![0.0; m];
        let mut c = vec![0.0; m];
        let mut rhs = vec![0.0; m];
        let col = |j: usize| -> [f64; 3] {
            // Column j of Q has entries at rows j, j+1, j+2.
            [1.0 / h[j], -1.0 / h[j] - 1.0 / h[j + 1], 1.0 / h[j + 1]]
        };
        for j in 0..m {
            let q = col(j);
            a[j] = (h[j] + h[j + 1]) / 3.0
                + lambda * (q[0] * q[0] / w[j] + q[1] * q[1] / w[j + 1] + q[2] * q[2] / w[j + 2]);
            if j + 1 < m {
                let p = col(j + 1);
                b[j] = h[j + 1] / 6.0 + lambda * (q[1] * p[0] / w[j + 1] + q[2] * p[1] / w[j + 2]);
            }
            if j + 2 < m {
                let p = col(j + 2);
                c[j] = lambda * (q[2] * p[0] / w[j + 2]);
            }
            rhs[j] = q[0] * ys[j] + q[1] * ys[j + 1] + q[2] * ys[j + 2];
        }
        let gamma_inner = solve_pentadiagonal(&a, &b, &c, &rhs);
        let mut g = ys.to_vec();
        for j in 0..m {
            let q = col(j);
            let v = gamma_inner[j] * lambda;
            g[j] -= v * q[0] / w[j];
            g[j + 1] -= v * q[1] / w[j + 1];
            g[j + 2] -= v * q[2] / w[j + 2];
        }
        let mut gamma = vec![0.0; n];
        gamma[1..n - 1].copy_from_slice(&gamma_inner);
        Spline { x0, span, x, g, gamma }
    }

    pub fn eval(&self, x_in: f64) -> f64 {
        let x = (x_in - self.x0) / self.span;
        let n = self.x.len();
        if x <= self.x[0] {
            let h = self.x[1] - self.x[0];
            let slope = (self.g[1] - self.g[0]) / h - h * self.gamma[1] / 6.0;
            return self.g[0] + slope * (x - self.x[0]);
        }
        if x >= self.x[n - 1] {
            let h = self.x[n - 1] - self.x[n - 2];
            let slope = (self.g[n - 1] - self.g[n - 2]) / h + h * self.gamma[n - 2] / 6.0;
            return self.g[n - 1] + slope * (x - self.x[n - 1]);
        }
        let i = self.x.partition_point(|p| *p <= x) - 1;
        let h = self.x[i + 1] - self.x[i];
        let (l, r) = (x - self.x[i], self.x[i + 1] - x);
        (l * self.g[i + 1] + r * self.g[i]) / h
            - l * r / 6.0 * ((1.0 + l / h) * self.gamma[i + 1] + (1.0 + r / h) * self.gamma[i])
    }
}

/// Solve a symmetric pentadiagonal system by LDL^T.
fn solve_pentadiagonal(a: &[f64], b: &[f64], c: &[f64], rhs: &[f64]) -> Vec<f64> {
    let n = a.len();
    let mut d = vec![0.0; n];
    let mut l1 = vec![0.0; n];
    let mut l2 = vec![0.0; n];
    for i in 0..n {
        let mut di = a[i];
        if i >= 1 {
            di -= l1[i - 1] * l1[i - 1] * d[i - 1];
        }
        if i >= 2 {
            di -= l2[i - 2] * l2[i - 2] * d[i - 2];
        }
        d[i] = if di.abs() < 1e-300 { 1e-300 } else { di };
        if i + 1 < n {
            let mut v = b[i];
            if i >= 1 {
                v -= l2[i - 1] * l1[i - 1] * d[i - 1];
            }
            l1[i] = v / d[i];
        }
        if i + 2 < n {
            l2[i] = c[i] / d[i];
        }
    }
    let mut z = rhs.to_vec();
    for i in 0..n {
        if i >= 1 {
            z[i] -= l1[i - 1] * z[i - 1];
        }
        if i >= 2 {
            z[i] -= l2[i - 2] * z[i - 2];
        }
    }
    for i in 0..n {
        z[i] /= d[i];
    }
    for i in (0..n).rev() {
        if i + 1 < n {
            z[i] -= l1[i] * z[i + 1];
        }
        if i + 2 < n {
            z[i] -= l2[i] * z[i + 2];
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_forward_holds_the_last_value() {
        let c = fit(InterpKind::StepForward, &[0.0, 10.0], &[1.0, 2.0], &[0.1, 0.1]).unwrap();
        assert_eq!(c.eval(-5.0), 1.0);
        assert_eq!(c.eval(5.0), 1.0);
        assert_eq!(c.eval(12.0), 2.0);
    }

    #[test]
    fn very_smooth_spline_becomes_the_straight_line() {
        let x: Vec<f64> = (0..20).map(|i| i as f64 * 10.0).collect();
        let y: Vec<f64> = x
            .iter()
            .enumerate()
            .map(|(i, v)| 3.0 + 0.5 * v + if i % 2 == 0 { 0.4 } else { -0.4 })
            .collect();
        let s = vec![0.4; 20];
        let c = fit(InterpKind::SmoothFixed(6.0), &x, &y, &s).unwrap();
        for probe in [15.0, 95.0, 175.0] {
            assert!((c.eval(probe) - (3.0 + 0.5 * probe)).abs() < 0.2);
        }
    }

    #[test]
    fn nearly_unsmoothed_spline_passes_through_points() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [1.0, 3.0, 2.0, 5.0, 4.0, 6.0];
        let s = [1.0; 6];
        let c = fit(InterpKind::SmoothFixed(-10.0), &x, &y, &s).unwrap();
        for i in 0..6 {
            assert!((c.eval(x[i]) - y[i]).abs() < 1e-3);
        }
    }

    #[test]
    fn auto_spline_tracks_smooth_drift_through_noise() {
        let x: Vec<f64> = (0..40).map(|i| i as f64 * 60.0).collect();
        let truth = |t: f64| 1.0 + 0.1 * (t / 900.0).sin();
        let y: Vec<f64> = x
            .iter()
            .enumerate()
            .map(|(i, t)| truth(*t) + 0.004 * (((i * 7919) % 13) as f64 / 6.0 - 1.0))
            .collect();
        let s = vec![0.004; 40];
        let c = fit(InterpKind::SmoothAuto, &x, &y, &s).unwrap();
        for t in [300.0, 900.0, 1500.0, 2100.0] {
            assert!((c.eval(t) - truth(t)).abs() < 0.006, "at {t}");
        }
    }
}
