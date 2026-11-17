// SPDX-License-Identifier: GPL-3.0-or-later
//! Chemical (CHIME-style) isochrons: York regression of PbO against ThO2* or
//! UO2* (Suzuki & Dunkley 2014), with the full-model age uncertainty and
//! leave-one-out outlier rejection.

use crate::constants::{DecayConstants, DEFAULT_SIGMA_FALLBACK_REL_PB, DEFAULT_SIGMA_FALLBACK_REL_TH_U};
use crate::row::*;
use crate::stats::{inflate_for_overdispersion, long_term_rel_2s_pct, pbo_below_detection_limit};

pub const W_TH: f64 = 264.0367;
pub const W_U: f64 = 270.0277;
pub const W_PB: f64 = 223.1994;
const MAX_AGE_ITERATIONS: usize = 100;

fn params(c: &DecayConstants) -> (f64, f64, f64, f64) {
    let r = if c.u235_frac != 0.0 { c.u238_frac / c.u235_frac } else { 137.88 };
    (c.lambda_232, c.lambda_235, c.lambda_238, r)
}

#[derive(Clone, Debug, Default)]
pub struct York {
    pub slope: f64,
    pub intercept: f64,
    pub slope_2s: f64,
    pub intercept_2s: f64,
    pub mswd: Option<f64>,
    pub n: usize,
    pub sum_w: f64,
    pub x_bar_adj: f64,
    pub slope_var: f64,
}

/// York (1966) regression with errors in x and y; uncertainties are 2-sigma.
pub fn york_regression(x: &[f64], sx: &[f64], y: &[f64], sy: &[f64], r_xy: f64) -> Option<York> {
    let n = x.len();
    if n < 2 {
        return None;
    }
    let sx: Vec<f64> = sx.iter().map(|v| v.max(1e-12)).collect();
    let sy: Vec<f64> = sy.iter().map(|v| v.max(1e-12)).collect();
    let wx: Vec<f64> = sx.iter().map(|s| 1.0 / (s * s)).collect();
    let wy: Vec<f64> = sy.iter().map(|s| 1.0 / (s * s)).collect();
    let mx = x.iter().sum::<f64>() / n as f64;
    let my = y.iter().sum::<f64>() / n as f64;
    let num0: f64 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let den0: f64 = x.iter().map(|a| (a - mx).powi(2)).sum();
    let mut slope = if den0 != 0.0 { num0 / den0 } else { 0.0 };
    let mut w = wx.clone();
    let mut beta = vec![0.0; n];
    let (mut xb, mut yb) = (mx, my);
    for _ in 0..200 {
        let alpha: Vec<f64> = wx.iter().zip(&wy).map(|(a, b)| (a * b).sqrt()).collect();
        w = (0..n).map(|i| wx[i] * wy[i] / (wx[i] + slope * slope * wy[i] - 2.0 * slope * r_xy * alpha[i])).collect();
        let sw: f64 = w.iter().sum();
        if sw == 0.0 {
            break;
        }
        xb = w.iter().zip(x).map(|(a, b)| a * b).sum::<f64>() / sw;
        yb = w.iter().zip(y).map(|(a, b)| a * b).sum::<f64>() / sw;
        let u: Vec<f64> = x.iter().map(|v| v - xb).collect();
        let v: Vec<f64> = y.iter().map(|v| v - yb).collect();
        beta = (0..n)
            .map(|i| w[i] * (u[i] / wy[i] + slope * v[i] / wx[i] - (slope * u[i] + v[i]) * r_xy / alpha[i]))
            .collect();
        let nb: f64 = (0..n).map(|i| w[i] * beta[i] * v[i]).sum();
        let db: f64 = (0..n).map(|i| w[i] * beta[i] * u[i]).sum();
        if db == 0.0 {
            break;
        }
        let ns = nb / db;
        if (ns - slope).abs() < 1e-15 {
            slope = ns;
            break;
        }
        slope = ns;
    }
    let intercept = yb - slope * xb;
    let cu: Vec<f64> = beta.iter().map(|b| xb + b).collect();
    let sw: f64 = w.iter().sum();
    if sw == 0.0 {
        return None;
    }
    let ub = w.iter().zip(&cu).map(|(a, b)| a * b).sum::<f64>() / sw;
    let d2: f64 = w.iter().zip(&cu).map(|(a, b)| a * (b - ub).powi(2)).sum();
    if d2 == 0.0 {
        return None;
    }
    let s2 = 1.0 / d2;
    let i2 = 1.0 / sw + ub * ub * s2;
    let mswd = if n > 2 {
        Some((0..n).map(|i| w[i] * (y[i] - slope * x[i] - intercept).powi(2)).sum::<f64>() / (n - 2) as f64)
    } else {
        None
    };
    Some(York {
        slope,
        intercept,
        slope_2s: 2.0 * s2.max(0.0).sqrt(),
        intercept_2s: 2.0 * i2.max(0.0).sqrt(),
        mswd,
        n,
        sum_w: sw,
        x_bar_adj: ub,
        slope_var: s2,
    })
}

fn u_star_fn(t: f64, target: f64, l235: f64, l238: f64, r: f64) -> f64 {
    ((l235 * t).exp() + r * (l238 * t).exp()) / (1.0 + r) - 1.0 - target
}

/// Th: slope of PbO vs ThO2*; U: slope of PbO vs UO2*.
pub fn slope_to_age_years(slope: f64, th_form: bool, c: &DecayConstants) -> Option<f64> {
    let (l232, l235, l238, r) = params(c);
    if th_form {
        let v = slope * (W_TH / W_PB) + 1.0;
        if v <= 0.0 {
            return None;
        }
        return Some(v.ln() / l232);
    }
    let target = slope * (W_U / W_PB);
    let (mut lo, mut hi) = (0.0, 5.0e10);
    let res = |t: f64| u_star_fn(t, target, l235, l238, r);
    if res(lo) * res(hi) > 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if res(lo) * res(mid) <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 1.0 {
            break;
        }
    }
    Some((lo + hi) / 2.0)
}

pub fn slope_age_2s_years(slope: f64, slope_2s: f64, th_form: bool, c: &DecayConstants) -> Option<f64> {
    let t0 = slope_to_age_years(slope, th_form, c)?;
    let eps = (slope.abs() * 1e-6).max(1e-9);
    let t1 = slope_to_age_years(slope + eps, th_form, c)?;
    Some(((t1 - t0) / eps * slope_2s).abs())
}

fn e_terms(t: f64, c: &DecayConstants) -> (f64, f64) {
    let (l232, l235, l238, r) = params(c);
    let e232 = (l232 * t).exp() - 1.0;
    let e235238 = ((l235 * t).exp() + r * (l238 * t).exp()) / (1.0 + r) - 1.0;
    (e232, e235238)
}

pub fn th_u_star(th: f64, u: f64, t: f64, c: &DecayConstants) -> (f64, f64) {
    let (e232, e235238) = e_terms(t, c);
    let ths = if e232 == 0.0 { th } else { th + u * (W_TH / W_U) * e235238 / e232 };
    let us = if e235238 == 0.0 { u } else { u + th * (W_U / W_TH) * e232 / e235238 };
    (ths, us)
}

pub fn th_u_star_sigma(s_th: f64, s_u: f64, t: f64, c: &DecayConstants) -> (f64, f64) {
    let (e232, e235238) = e_terms(t, c);
    let ku = if e232 != 0.0 { (W_TH / W_U) * e235238 / e232 } else { 0.0 };
    let kth = if e235238 != 0.0 { (W_U / W_TH) * e232 / e235238 } else { 0.0 };
    ((s_th * s_th + (ku * s_u).powi(2)).sqrt(), (s_u * s_u + (kth * s_th).powi(2)).sqrt())
}

#[allow(clippy::too_many_arguments)]
pub fn full_model_age_2s_years(
    t: f64,
    p: f64,
    th: &[f64],
    u: &[f64],
    pb: &[f64],
    sth: &[f64],
    su: &[f64],
    spb: &[f64],
    c: &DecayConstants,
    sig_t: f64,
    sig_p: f64,
) -> Option<f64> {
    let (l232, l235, l238, r) = params(c);
    let misfit = |t: f64, pi: f64| -> f64 {
        let a = (W_PB / W_TH) * ((l232 * t).exp() - 1.0);
        let b = (W_PB / W_U) * (((l235 * t).exp() + r * (l238 * t).exp()) / (1.0 + r) - 1.0);
        let mut tot = 0.0;
        for i in 0..th.len() {
            let res = pb[i] - a * th[i] - b * u[i] - pi;
            tot += res * res / (spb[i] * spb[i] + a * a * sth[i] * sth[i] + b * b * su[i] * su[i]);
        }
        tot
    };
    let ht = (sig_t.abs() * 0.05).max(1.0e3);
    let hp = (sig_p.abs() * 0.05).max(1.0e-9);
    let f0 = misfit(t, p);
    let htt = (misfit(t + ht, p) - 2.0 * f0 + misfit(t - ht, p)) / (ht * ht);
    let hpp = (misfit(t, p + hp) - 2.0 * f0 + misfit(t, p - hp)) / (hp * hp);
    let htp = (misfit(t + ht, p + hp) - misfit(t + ht, p - hp) - misfit(t - ht, p + hp) + misfit(t - ht, p - hp))
        / (4.0 * ht * hp);
    let det = htt * hpp - htp * htp;
    if !(htt > 0.0 && hpp > 0.0 && det > 0.0) {
        return None;
    }
    Some(2.0 * (2.0 * hpp / det).sqrt())
}

/// (ThO2, UO2, PbO) wt% of a processed or raw row.
pub fn extract_th_u_pb(row: &Row) -> Option<(f64, f64, f64)> {
    let get = |a: &str, b: &str| -> Option<f64> {
        let key = if row.contains_key(a) { a } else { b };
        match row.get(key) {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) if s.is_empty() => None,
            Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
            Some(serde_json::Value::Number(n)) => n.as_f64(),
            Some(serde_json::Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    };
    Some((get("ThO2_wt%", "ThO2")?, get("UO2_wt%", "UO2")?, get("PbO_wt%", "PbO")?))
}

fn measured_sigma(row: &Row, key: &str) -> Option<f64> {
    num_loose(row, key).filter(|v| *v > 0.0 && v.is_finite())
}

pub fn point_sigmas(row: &Row, th: f64, u: f64, pb: f64, f_thu: f64, f_pb: f64) -> (f64, f64, f64) {
    (
        measured_sigma(row, "sigma_ThO2").unwrap_or(th * f_thu).max(1e-6),
        measured_sigma(row, "sigma_UO2").unwrap_or(u * f_thu).max(1e-6),
        measured_sigma(row, "sigma_PbO").unwrap_or(pb * f_pb).max(1e-7),
    )
}

pub fn dln_pb_dln_t(th: f64, u: f64, t: f64, c: &DecayConstants) -> f64 {
    let (l232, l235, l238, r) = params(c);
    let a = (W_PB / W_TH) * ((l232 * t).exp() - 1.0);
    let b = (W_PB / W_U) * (((l235 * t).exp() + r * (l238 * t).exp()) / (1.0 + r) - 1.0);
    let da = (W_PB / W_TH) * l232 * (l232 * t).exp();
    let db = (W_PB / W_U) * (l235 * (l235 * t).exp() + r * l238 * (l238 * t).exp()) / (1.0 + r);
    let pbv = a * th + b * u;
    if pbv > 0.0 {
        t * (da * th + db * u) / pbv
    } else {
        1.0
    }
}

pub fn point_sigmas_with_excess(row: &Row, th: f64, u: f64, pb: f64, c: &DecayConstants, f_thu: f64, f_pb: f64) -> (f64, f64, f64) {
    let (s_th, s_u, mut s_pb) = point_sigmas(row, th, u, pb, f_thu, f_pb);
    if let (Some(ex), Some(age)) = (num(row, "Excess_2s_rel_pct"), num(row, "Age_Ma")) {
        if ex > 0.0 && age > 0.0 {
            let se = ex / 200.0 * dln_pb_dln_t(th, u, age * 1e6, c) * pb;
            s_pb = (s_pb * s_pb + se * se).sqrt();
        }
    }
    (s_th, s_u, s_pb)
}

#[derive(Clone, Debug, Default)]
pub struct Isochron {
    pub th_form: bool,
    pub n: usize,
    pub slope: f64,
    pub slope_2s: f64,
    pub intercept: f64,
    pub intercept_2s: f64,
    pub intercept_2s_internal: f64,
    pub mswd: Option<f64>,
    pub mswd_expanded: bool,
    pub p_value: f64,
    pub age_ma: Option<f64>,
    /// Total (with the systematic term once).
    pub age_2s_ma: Option<f64>,
    pub age_2s_internal_ma: Option<f64>,
    pub age_2s_analytical_ma: Option<f64>,
    pub age_2s_plain_ma: Option<f64>,
    pub long_term_2s_ma: f64,
    /// "full model" or "slope".
    pub age_2s_method: String,
    pub x_star: Vec<f64>,
    pub sx_star: Vec<f64>,
    pub y_pb: Vec<f64>,
    pub sy_pb: Vec<f64>,
    /// Indices into the input rows.
    pub used: Vec<usize>,
    pub rejected: Vec<usize>,
    pub bdl: Vec<usize>,
    pub n_candidates: usize,
    pub rejection_sigma: Option<f64>,
}

struct Pt {
    idx: usize,
    th: f64,
    u: f64,
    pb: f64,
    sth: f64,
    su: f64,
    spb: f64,
}

fn fit(points: &[Pt], c: &DecayConstants, th_form: bool) -> Option<Isochron> {
    if points.len() < 3 {
        return None;
    }
    let star = |t: f64| -> (Vec<f64>, Vec<f64>) {
        let x = points.iter().map(|p| {
            let s = th_u_star(p.th, p.u, t, c);
            if th_form { s.0 } else { s.1 }
        });
        let sx = points.iter().map(|p| {
            let s = th_u_star_sigma(p.sth, p.su, t, c);
            if th_form { s.0 } else { s.1 }
        });
        (x.collect(), sx.collect())
    };
    let pb: Vec<f64> = points.iter().map(|p| p.pb).collect();
    let spb: Vec<f64> = points.iter().map(|p| p.spb).collect();
    let (mut xs, mut sxs) = star(5.0e8);
    let mut reg = york_regression(&xs, &sxs, &pb, &spb, 0.0)?;
    let mut t_prev = 5.0e8;
    for _ in 0..MAX_AGE_ITERATIONS {
        let Some(te) = slope_to_age_years(reg.slope, th_form, c) else { break };
        if te <= 0.0 || (te - t_prev).abs() < 1.0 {
            break;
        }
        let (nx, nsx) = star(te);
        let Some(nr) = york_regression(&nx, &nsx, &pb, &spb, 0.0) else { break };
        xs = nx;
        sxs = nsx;
        reg = nr;
        t_prev = te;
    }
    let age = slope_to_age_years(reg.slope, th_form, c);
    let sa = slope_age_2s_years(reg.slope, reg.slope_2s, th_form, c);
    let (mut a2s, mut method) = (sa, "slope");
    if let (Some(t), Some(s)) = (age, sa) {
        if t > 0.0 && s != 0.0 {
            let th: Vec<f64> = points.iter().map(|p| p.th).collect();
            let u: Vec<f64> = points.iter().map(|p| p.u).collect();
            let sth: Vec<f64> = points.iter().map(|p| p.sth).collect();
            let su: Vec<f64> = points.iter().map(|p| p.su).collect();
            if let Some(f) = full_model_age_2s_years(t, reg.intercept, &th, &u, &pb, &sth, &su, &spb, c, s / 2.0, reg.intercept_2s / 2.0) {
                a2s = Some(f);
                method = "full model";
            }
        }
    }
    let (inflated, p) = inflate_for_overdispersion(reg.mswd, reg.n as i64 - 2);
    let factor = if inflated { reg.mswd.unwrap().sqrt() } else { 1.0 };
    Some(Isochron {
        th_form,
        n: reg.n,
        slope: reg.slope,
        slope_2s: reg.slope_2s,
        intercept: reg.intercept,
        intercept_2s: reg.intercept_2s * factor,
        intercept_2s_internal: reg.intercept_2s,
        mswd: reg.mswd,
        mswd_expanded: factor > 1.0,
        p_value: p,
        age_ma: age.map(|t| t / 1e6),
        age_2s_ma: a2s.map(|v| v * factor / 1e6),
        age_2s_internal_ma: a2s.map(|v| v / 1e6),
        age_2s_method: method.into(),
        x_star: xs,
        sx_star: sxs,
        y_pb: pb,
        sy_pb: spb,
        ..Default::default()
    })
}

fn loo_residuals(xs: &[f64], sxs: &[f64], ys: &[f64], sys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    (0..n)
        .map(|i| {
            let keep: Vec<usize> = (0..n).filter(|&j| j != i).collect();
            let pick = |v: &[f64]| keep.iter().map(|&j| v[j]).collect::<Vec<f64>>();
            match york_regression(&pick(xs), &pick(sxs), &pick(ys), &pick(sys), 0.0) {
                None => 0.0,
                Some(r) => {
                    let pv = 1.0 / r.sum_w + (xs[i] - r.x_bar_adj).powi(2) * r.slope_var;
                    let d = (sys[i] * sys[i] + r.slope * r.slope * sxs[i] * sxs[i] + pv).sqrt();
                    (ys[i] - r.intercept - r.slope * xs[i]).abs() / d
                }
            }
        })
        .collect()
}

pub fn compute_isochron(rows: &[&Row], c: &DecayConstants, th_form: bool, reject: bool, k: f64) -> Option<Isochron> {
    compute_isochron_with(rows, c, th_form, DEFAULT_SIGMA_FALLBACK_REL_TH_U, DEFAULT_SIGMA_FALLBACK_REL_PB, reject, k)
}

pub fn compute_isochron_with(
    rows: &[&Row],
    c: &DecayConstants,
    th_form: bool,
    f_thu: f64,
    f_pb: f64,
    reject: bool,
    k: f64,
) -> Option<Isochron> {
    let mut pts = Vec::new();
    let mut bdl = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if pbo_below_detection_limit(row) {
            bdl.push(i);
            continue;
        }
        let Some((th, u, pb)) = extract_th_u_pb(row) else { continue };
        let (sth, su, spb) = point_sigmas_with_excess(row, th, u, pb, c, f_thu, f_pb);
        pts.push(Pt { idx: i, th, u, pb, sth, su, spb });
    }
    let n_candidates = pts.len();
    let mut rejected = Vec::new();
    let mut result;
    loop {
        result = fit(&pts, c, th_form)?;
        if !reject || pts.len() <= 3 {
            break;
        }
        let res = loo_residuals(&result.x_star, &result.sx_star, &result.y_pb, &result.sy_pb);
        let mut worst = 0;
        for i in 1..res.len() {
            if res[i] > res[worst] {
                worst = i;
            }
        }
        if res[worst] <= k {
            break;
        }
        rejected.push(pts.remove(worst).idx);
    }
    if let (Some(age), Some(a2s)) = (result.age_ma, result.age_2s_ma) {
        let used: Vec<&Row> = pts.iter().map(|p| rows[p.idx]).collect();
        let lt = long_term_rel_2s_pct(&used) / 100.0 * age;
        result.age_2s_analytical_ma = Some(a2s);
        result.long_term_2s_ma = lt;
        result.age_2s_ma = Some((a2s * a2s + lt * lt).sqrt());
        result.age_2s_plain_ma = result.age_2s_internal_ma.map(|i| (i * i + lt * lt).sqrt());
    } else {
        result.age_2s_analytical_ma = result.age_2s_ma;
        result.long_term_2s_ma = 0.0;
    }
    result.rejected = rejected;
    result.used = pts.iter().map(|p| p.idx).collect();
    result.bdl = bdl;
    result.n_candidates = n_candidates;
    result.rejection_sigma = if reject { Some(k) } else { None };
    Some(result)
}

/// 95 % joint confidence semi-axis multiplier, sqrt(-2 ln 0.05).
pub fn error_ellipse_sigma() -> f64 {
    (-2.0 * 0.05f64.ln()).sqrt()
}
