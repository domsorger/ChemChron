// SPDX-License-Identifier: GPL-3.0-or-later
//! Decay equations, ages, concordia, concordia age, Stacey-Kramers common Pb
//! and the 230Th disequilibrium term. Ages are in Ma; decay constants are per year.

use crate::stats::chi2_sf;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecayConstants {
    pub l238: f64,
    pub l235: f64,
    pub l232: f64,
    pub l230: f64,
    /// Present-day 238U/235U.
    pub u85: f64,
}

impl Default for DecayConstants {
    /// Steiger & Jaeger (1977) for 238U, 235U and 232Th; 238U/235U from
    /// Hiess et al. (2012); 230Th from Cheng et al. (2013). Editable per session.
    fn default() -> Self {
        DecayConstants {
            l238: 1.55125e-10,
            l235: 9.8485e-10,
            l232: 4.9475e-11,
            l230: 9.1705e-6,
            u85: 137.818,
        }
    }
}

impl DecayConstants {
    pub fn r68(&self, t_ma: f64) -> f64 {
        (self.l238 * t_ma * 1e6).exp_m1()
    }
    pub fn r75(&self, t_ma: f64) -> f64 {
        (self.l235 * t_ma * 1e6).exp_m1()
    }
    pub fn r82(&self, t_ma: f64) -> f64 {
        (self.l232 * t_ma * 1e6).exp_m1()
    }
    /// Radiogenic 207Pb/206Pb at age t.
    pub fn r76(&self, t_ma: f64) -> f64 {
        if t_ma.abs() < 1e-6 {
            return self.l235 / self.l238 / self.u85;
        }
        self.r75(t_ma) / self.r68(t_ma) / self.u85
    }
    pub fn age68(&self, r: f64) -> f64 {
        (1.0 + r).ln() / self.l238 / 1e6
    }
    pub fn age75(&self, r: f64) -> f64 {
        (1.0 + r).ln() / self.l235 / 1e6
    }
    pub fn age82(&self, r: f64) -> f64 {
        (1.0 + r).ln() / self.l232 / 1e6
    }
    /// 207Pb/206Pb age by bisection (r76 rises monotonically with age).
    pub fn age76(&self, r: f64) -> f64 {
        if !r.is_finite() || r <= self.r76(0.0) {
            return f64::NAN;
        }
        let (mut lo, mut hi) = (0.0, 7000.0);
        if r >= self.r76(hi) {
            return f64::NAN;
        }
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if self.r76(mid) < r {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
    /// Age uncertainty from a ratio uncertainty, with the slope taken at the ratio.
    pub fn sigma_age68(&self, r: f64, s: f64) -> f64 {
        s / (1.0 + r) / self.l238 / 1e6
    }
    pub fn sigma_age75(&self, r: f64, s: f64) -> f64 {
        s / (1.0 + r) / self.l235 / 1e6
    }
    pub fn sigma_age82(&self, r: f64, s: f64) -> f64 {
        s / (1.0 + r) / self.l232 / 1e6
    }
    pub fn sigma_age76(&self, r: f64, s: f64) -> f64 {
        let t = self.age76(r);
        if !t.is_finite() {
            return f64::NAN;
        }
        let dt = (t * 1e-4).max(0.01);
        let slope = (self.r76(t + dt) - self.r76((t - dt).max(0.0))) / (dt + dt.min(t));
        s / slope
    }
}

/// Stacey & Kramers (1975) two-stage model: (206Pb/204Pb, 207Pb/204Pb, 208Pb/204Pb) at t.
pub fn stacey_kramers(t_ma: f64, d: &DecayConstants) -> (f64, f64, f64) {
    let t = t_ma.clamp(0.0, 3700.0) * 1e6;
    let t0 = 3.7e9;
    let e = |l: f64| (l * t0).exp() - (l * t).exp();
    let a = 11.152 + 9.735 * e(d.l238);
    let b = 12.998 + 9.735 / 137.88 * e(d.l235);
    let c = 31.23 + 36.84 * e(d.l232);
    (a, b, c)
}

/// One analysis on a concordia diagram.
#[derive(Clone, Copy, Debug)]
pub struct ConcordiaPoint {
    pub x: f64,
    pub y: f64,
    /// 2 sigma.
    pub sx: f64,
    pub sy: f64,
    pub rho: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConcordiaAge {
    pub age: f64,
    /// 2 sigma.
    pub unc2s: f64,
    pub mswd: f64,
    pub p: f64,
    pub n: usize,
}

/// Concordia age of analyses on the Wetherill diagram (x = 207Pb/235U,
/// y = 206Pb/238U), after Ludwig (1998): weighted mean composition, then the
/// age on the curve closest to it, with an MSWD of equivalence + concordance.
pub fn concordia_age(points: &[ConcordiaPoint], d: &DecayConstants) -> Option<ConcordiaAge> {
    let pts: Vec<&ConcordiaPoint> = points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.sx > 0.0 && p.sy > 0.0)
        .collect();
    if pts.is_empty() {
        return None;
    }
    // Inverse covariance of each analysis, with sigma = 2 sigma / 2.
    let inv = |p: &ConcordiaPoint| -> [f64; 3] {
        let (sx, sy) = (p.sx / 2.0, p.sy / 2.0);
        let r = p.rho.clamp(-0.999, 0.999);
        let k = 1.0 / (1.0 - r * r);
        [k / (sx * sx), -k * r / (sx * sy), k / (sy * sy)]
    };
    let (mut oxx, mut oxy, mut oyy, mut bx, mut by) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for p in &pts {
        let w = inv(p);
        oxx += w[0];
        oxy += w[1];
        oyy += w[2];
        bx += w[0] * p.x + w[1] * p.y;
        by += w[1] * p.x + w[2] * p.y;
    }
    let det = oxx * oyy - oxy * oxy;
    if det <= 0.0 {
        return None;
    }
    let mx = (oyy * bx - oxy * by) / det;
    let my = (-oxy * bx + oxx * by) / det;
    let mut s_eq = 0.0;
    for p in &pts {
        let w = inv(p);
        let (dx, dy) = (p.x - mx, p.y - my);
        s_eq += w[0] * dx * dx + 2.0 * w[1] * dx * dy + w[2] * dy * dy;
    }
    let s = |t: f64| -> f64 {
        let (dx, dy) = (mx - d.r75(t), my - d.r68(t));
        oxx * dx * dx + 2.0 * oxy * dx * dy + oyy * dy * dy
    };
    let start = d.age68(my);
    if !start.is_finite() {
        return None;
    }
    let (mut lo, mut hi) = ((start * 0.5).max(0.001), (start * 1.5).max(1.0));
    let phi = 0.618_033_988_749_895;
    for _ in 0..200 {
        let a = hi - phi * (hi - lo);
        let b = lo + phi * (hi - lo);
        if s(a) < s(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    let t = 0.5 * (lo + hi);
    let h = (t * 1e-3).max(1e-3);
    let curv = (s(t + h) - 2.0 * s(t) + s(t - h)) / (h * h);
    if curv <= 0.0 {
        return None;
    }
    let sigma = (2.0 / curv).sqrt();
    let dof = (2 * pts.len() - 1) as f64;
    let total = s_eq + s(t);
    Some(ConcordiaAge {
        age: t,
        unc2s: 2.0 * sigma,
        mswd: total / dof,
        p: chi2_sf(total, dof),
        n: pts.len(),
    })
}

/// Outline of a 2 sigma error ellipse, as points in data units.
pub fn ellipse(p: &ConcordiaPoint, segments: usize) -> Vec<(f64, f64)> {
    let r = p.rho.clamp(-0.999, 0.999);
    // The 2 sigma of a single ratio already sets the half-widths of the box.
    (0..=segments)
        .map(|i| {
            let a = i as f64 / segments as f64 * std::f64::consts::TAU;
            let u = a.cos();
            let v = r * a.cos() + (1.0 - r * r).sqrt() * a.sin();
            (p.x + p.sx * u, p.y + p.sy * v)
        })
        .collect()
}

/// Discordance in percent. Below 1000 Ma the 206Pb/238U age is compared with
/// the 207Pb/235U age, above with the 207Pb/206Pb age.
pub fn discordance(a68: f64, a75: f64, a76: f64) -> f64 {
    let reference = if a68 < 1000.0 || !a76.is_finite() { a75 } else { a76 };
    100.0 * (1.0 - a68 / reference)
}

/// 206Pb/238U ratio with the 207Pb-based common-Pb correction: the line from
/// common 207Pb/206Pb through the measured point, intersected with concordia.
/// Returns (corrected 206Pb/238U, age, fraction of 206Pb that is common).
pub fn correct_207(
    r68: f64,
    r76: f64,
    common_76: impl Fn(f64) -> f64,
    d: &DecayConstants,
) -> Option<(f64, f64, f64)> {
    let mut t = d.age68(r68);
    for _ in 0..60 {
        let star = d.r76(t.max(0.01));
        let c = common_76(t);
        if (c - star).abs() < 1e-9 {
            return None;
        }
        let f = ((r76 - star) / (c - star)).clamp(-0.5, 1.0);
        let corrected = r68 * (1.0 - f);
        if corrected <= 0.0 {
            return None;
        }
        let next = d.age68(corrected);
        if (next - t).abs() < 1e-5 {
            return Some((corrected, next, f));
        }
        t = 0.5 * (t + next);
    }
    None
}

/// 208Pb-based common-Pb correction: the common 206Pb/238U `x` for which the
/// 206Pb/238U and 208Pb/232Th ages agree once common lead with 208Pb/206Pb =
/// `c86` is removed (Th-U concordance assumed). `th_u` is 232Th/238U.
/// Returns (x, common fraction of 206Pb).
pub fn correct_208(r68: f64, r82: f64, th_u: f64, c86: f64, d: &DecayConstants) -> Option<(f64, f64)> {
    if !(th_u > 0.0) || !r68.is_finite() || !r82.is_finite() {
        return None;
    }
    let gap = |x: f64| d.age68(r68 - x) - d.age82(r82 - x * c86 / th_u);
    let (mut lo, mut hi) = (-0.5 * r68, 0.95 * r68);
    if !(gap(lo) * gap(hi) < 0.0) {
        return None;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if gap(lo) * gap(mid) <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let x = 0.5 * (lo + hi);
    Some((x, x / r68))
}

/// 230Th disequilibrium correction of 206Pb/238U after Schaerer (1984):
/// subtracts (l238/l230) * (f - 1), with f = (Th/U mineral) / (Th/U melt).
pub fn th230_corrected_r68(r68: f64, th_u_mineral: f64, th_u_melt: f64, d: &DecayConstants) -> f64 {
    let f = th_u_mineral / th_u_melt;
    r68 - d.l238 / d.l230 * (f - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_round_trip_through_ratios() {
        let d = DecayConstants::default();
        for t in [50.0, 424.9, 1062.4, 2700.0] {
            assert!((d.age68(d.r68(t)) - t).abs() < 1e-8);
            assert!((d.age75(d.r75(t)) - t).abs() < 1e-8);
            assert!((d.age82(d.r82(t)) - t).abs() < 1e-8);
            assert!((d.age76(d.r76(t)) - t).abs() < 1e-5);
        }
    }

    #[test]
    fn pb208_correction_recovers_a_known_age() {
        // A 450 Ma monazite with Th/U = 10 and 3 % common 206Pb of Stacey-Kramers composition.
        let d = DecayConstants::default();
        let t = 450.0;
        let (a, _, c) = stacey_kramers(t, &d);
        let c86 = c / a;
        let th_u = 10.0;
        let star68 = d.r68(t);
        let x = 0.03 / 0.97 * star68;
        let (m68, m82) = (star68 + x, d.r82(t) + x * c86 / th_u);
        let (fx, f) = correct_208(m68, m82, th_u, c86, &d).unwrap();
        assert!((fx - x).abs() / x < 1e-6, "{fx} {x}");
        assert!((f - 0.03).abs() < 1e-6, "{f}");
        assert!((d.age68(m68 - fx) - t).abs() < 0.01);
        // Without common lead nothing is taken away.
        let (x0, _) = correct_208(star68, d.r82(t), th_u, c86, &d).unwrap();
        assert!(x0.abs() < 1e-9 * star68);
        assert!(correct_208(star68, d.r82(t), 0.0, c86, &d).is_none());
    }

    #[test]
    fn th230_correction_shifts_the_age_by_f_minus_one_over_lambda230() {
        // Schaerer (1984): 206Pb/238U changes by (lambda238/lambda230)(f - 1),
        // f = (Th/U mineral) / (Th/U melt), so a young age by about
        // (f - 1)/lambda230: +87 ka for a zircon with f = 0.2, -0.98 Ma for a
        // monazite with f = 10 (a little less at older ages, by exp(lambda238 t)).
        let d = DecayConstants::default();
        for (t, f, shift_ka) in [(1.0, 0.2, 87.2), (1.0, 10.0, -981.4), (30.0, 10.0, -976.9), (500.0, 1.0, 0.0)] {
            let r = d.r68(t);
            let corrected = th230_corrected_r68(r, f * 4.0, 4.0, &d);
            let dt_ka = (d.age68(corrected) - t) * 1000.0;
            let expected = -(f - 1.0) / d.l230 / (d.l238 * t * 1e6).exp() / 1000.0;
            assert!((dt_ka - expected).abs() < 0.5, "t {t} f {f}: {dt_ka} ka vs {expected}");
            assert!((dt_ka - shift_ka).abs() < 0.5, "{dt_ka} vs {shift_ka}");
        }
    }

    #[test]
    fn published_zircon_ratio_gives_its_age() {
        // 91500: 206Pb/238U = 0.17917 corresponds to about 1062 Ma.
        let d = DecayConstants::default();
        assert!((d.age68(0.17917) - 1062.4).abs() < 0.5);
    }

    #[test]
    fn stacey_kramers_present_day_values() {
        let d = DecayConstants::default();
        let (a, b, c) = stacey_kramers(0.0, &d);
        assert!((a - 18.700).abs() < 0.03, "{a}");
        assert!((b - 15.628).abs() < 0.03, "{b}");
        assert!((c - 38.63).abs() < 0.05, "{c}");
    }

    #[test]
    fn concordia_age_finds_the_true_age() {
        let d = DecayConstants::default();
        let t = 424.9;
        let pts: Vec<ConcordiaPoint> = (0..20)
            .map(|i| {
                let e = if i % 2 == 0 { 1.0 } else { -1.0 };
                ConcordiaPoint {
                    x: d.r75(t) * (1.0 + e * 0.002),
                    y: d.r68(t) * (1.0 - e * 0.001),
                    sx: d.r75(t) * 0.02,
                    sy: d.r68(t) * 0.015,
                    rho: 0.6,
                }
            })
            .collect();
        let c = concordia_age(&pts, &d).unwrap();
        assert!((c.age - t).abs() < 0.5, "{}", c.age);
        assert!(c.unc2s > 0.0 && c.unc2s < 3.0, "{}", c.unc2s);
    }

    #[test]
    fn correction_207_recovers_age_of_a_mixture() {
        let d = DecayConstants::default();
        let t = 300.0;
        let c76 = 0.85;
        let f = 0.1;
        // Mixture on the Tera-Wasserburg line: 10 % of 206Pb is common.
        let r68_star = d.r68(t);
        let r68_meas = r68_star / (1.0 - f);
        let r76_meas = f * c76 + (1.0 - f) * d.r76(t);
        let (r, age, frac) = correct_207(r68_meas, r76_meas, |_| c76, &d).unwrap();
        assert!((age - t).abs() < 0.1, "{age}");
        assert!((frac - f).abs() < 1e-3);
        assert!((r - r68_star).abs() < 1e-4);
    }
}
