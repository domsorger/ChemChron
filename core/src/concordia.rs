// SPDX-License-Identifier: GPL-3.0-or-later
//! Concordia ages (Ludwig 1998) and discordia intercepts (York 2004) of
//! Tera-Wasserburg points `(x, x_2s, y, y_2s, rho)`.

use crate::constants::DecayConstants;
use crate::stats::{chi2_sf, inflate_for_overdispersion};
use crate::upb::U238_U235;

pub type Pt = (f64, f64, f64, f64, f64);
type M2 = [[f64; 2]; 2];
pub const T_MAX_YEARS: f64 = 4.6e9;

fn cov(sx2: f64, sy2: f64, rho: f64) -> M2 {
    let (sx, sy) = (sx2 / 2.0, sy2 / 2.0);
    let c = rho * sx * sy;
    [[sx * sx, c], [c, sy * sy]]
}

fn inv2(m: M2) -> M2 {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    [[m[1][1] / det, -m[0][1] / det], [-m[1][0] / det, m[0][0] / det]]
}

fn quad(v: (f64, f64), m: M2) -> f64 {
    v.0 * (m[0][0] * v.0 + m[0][1] * v.1) + v.1 * (m[1][0] * v.0 + m[1][1] * v.1)
}

pub fn golden_min(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, n_grid: usize, iters: usize) -> f64 {
    let ts: Vec<f64> = (0..=n_grid).map(|k| lo + (hi - lo) * k as f64 / n_grid as f64).collect();
    let vals: Vec<f64> = ts.iter().map(|t| f(*t)).collect();
    let mut k = 0;
    for i in 1..vals.len() {
        if vals[i] < vals[k] {
            k = i;
        }
    }
    let (mut l, mut r) = (ts[k.saturating_sub(1)], ts[(k + 1).min(n_grid)]);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..iters {
        let m1 = r - g * (r - l);
        let m2 = l + g * (r - l);
        if f(m1) < f(m2) {
            r = m2;
        } else {
            l = m1;
        }
    }
    0.5 * (l + r)
}

pub fn tw_point(t: f64, l8: f64, l5: f64) -> (f64, f64) {
    let (e8, e5) = ((l8 * t).exp_m1(), (l5 * t).exp_m1());
    (1.0 / e8, e5 / (U238_U235 * e8))
}

pub fn wetherill_point(t: f64, l8: f64, l5: f64) -> (f64, f64) {
    ((l5 * t).exp_m1(), (l8 * t).exp_m1())
}

/// (207/235, 2s, 206/238, 2s, rho) from a Tera-Wasserburg point.
pub fn tw_to_wetherill(p: Pt) -> Pt {
    let (x, sx2, y, sy2, rho) = p;
    let (r68, r75) = (1.0 / x, y * U238_U235 / x);
    let (a, b, c) = (sy2 / y, sx2 / x, rho);
    let rel75 = (a * a + b * b - 2.0 * c * a * b).max(0.0).sqrt();
    let rw = if b > 0.0 && rel75 > 0.0 { (b * b - c * a * b) / (b * rel75) } else { 0.0 };
    (r75, r75 * rel75, r68, r68 * b, rw)
}

pub fn wetherill_to_tw(p: Pt) -> Pt {
    let (r75, s75, r68, s68, rho) = p;
    let (x, y) = (1.0 / r68, r75 / (U238_U235 * r68));
    let (a, b, c) = (s75 / r75, s68 / r68, rho);
    let ry = (a * a + b * b - 2.0 * c * a * b).max(0.0).sqrt();
    let rt = if b > 0.0 && ry > 0.0 { (b * b - c * a * b) / (b * ry) } else { 0.0 };
    (x, x * b, y, y * ry, rt)
}

#[derive(Clone, Debug, Default)]
pub struct ConcordiaAge {
    pub age_ma: f64,
    pub age_2s_ma: f64,
    /// Wetherill (207/235, 206/238).
    pub mean: (f64, f64),
    pub mean_cov: [[f64; 2]; 2],
    pub n: usize,
    pub mswd_conc: f64,
    pub p_conc: f64,
    pub mswd_eq: f64,
    pub p_eq: f64,
    pub mswd: f64,
    pub p: f64,
    pub decay_errors: bool,
}

pub fn concordia_age(points: &[Pt], c: &DecayConstants, decay_rel_1s: Option<(f64, f64)>) -> Option<ConcordiaAge> {
    let pts: Vec<Pt> = points.iter().copied().filter(|p| p.0 > 0.0 && p.2 > 0.0 && p.1 > 0.0 && p.3 > 0.0).map(tw_to_wetherill).collect();
    let n = pts.len();
    if n < 1 {
        return None;
    }
    let (l8, l5) = (c.lambda_238, c.lambda_235);
    let om: Vec<M2> = pts.iter().map(|p| inv2(cov(p.1, p.3, p.4))).collect();
    let mut so = [[0.0; 2]; 2];
    for o in &om {
        for i in 0..2 {
            for j in 0..2 {
                so[i][j] += o[i][j];
            }
        }
    }
    let cz = inv2(so);
    let mut rhs = [0.0; 2];
    for (o, p) in om.iter().zip(&pts) {
        for i in 0..2 {
            rhs[i] += o[i][0] * p.0 + o[i][1] * p.2;
        }
    }
    let zx = cz[0][0] * rhs[0] + cz[0][1] * rhs[1];
    let zy = cz[1][0] * rhs[0] + cz[1][1] * rhs[1];
    let s_eq: f64 = om.iter().zip(&pts).map(|(o, p)| quad((p.0 - zx, p.2 - zy), *o)).sum();
    let total_cov = |t: f64| -> M2 {
        let Some((r8, r5)) = decay_rel_1s else { return cz };
        let (s8, s5) = (r8 * l8, r5 * l5);
        let jac = [[0.0, t * (l5 * t).exp()], [t * (l8 * t).exp(), 0.0]];
        let mut m = cz;
        for i in 0..2 {
            for j in 0..2 {
                m[i][j] += jac[i][0] * jac[j][0] * s8 * s8 + jac[i][1] * jac[j][1] * s5 * s5;
            }
        }
        m
    };
    let s_c = |t: f64| {
        let (cx, cy) = wetherill_point(t, l8, l5);
        quad((zx - cx, zy - cy), inv2(total_cov(t)))
    };
    let t68 = zy.ln_1p() / l8;
    let t = golden_min(&s_c, (0.5 * t68).max(1e5), (1.5 * t68 + 2e8).min(T_MAX_YEARS), 400, 80);
    let h = (t * 1e-4).max(1e3);
    let curv = (s_c(t + h) - 2.0 * s_c(t) + s_c(t - h)) / (h * h);
    let sig = if curv > 0.0 { (2.0 / curv).sqrt() } else { f64::NAN };
    let sc = s_c(t);
    let dof = 2 * n as i64 - 2;
    Some(ConcordiaAge {
        age_ma: t / 1e6,
        age_2s_ma: 2.0 * sig / 1e6,
        mean: (zx, zy),
        mean_cov: cz,
        n,
        mswd_conc: sc,
        p_conc: chi2_sf(sc, 1),
        mswd_eq: if dof > 0 { s_eq / dof as f64 } else { f64::NAN },
        p_eq: if dof > 0 { chi2_sf(s_eq, dof) } else { f64::NAN },
        mswd: (s_eq + sc) / (2 * n - 1) as f64,
        p: chi2_sf(s_eq + sc, 2 * n as i64 - 1),
        decay_errors: decay_rel_1s.is_some(),
    })
}

fn erfc(x: f64) -> f64 {
    // Numerical Recipes erfcc, relative error < 1.2e-7
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t
        * (-z * z - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07 + t * (-1.135_203_98 + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

pub fn rejection_distance(k: f64) -> f64 {
    (-2.0 * erfc(k / 2f64.sqrt()).ln()).sqrt()
}

/// Leave-one-out rejection for the concordia age: (kept, rejected) indices.
pub fn reject_concordia_outliers(points: &[Pt], k: f64, min_n: usize) -> (Vec<usize>, Vec<usize>) {
    let w: Vec<Pt> = points.iter().copied().map(tw_to_wetherill).collect();
    let om: Vec<M2> = w.iter().map(|p| inv2(cov(p.1, p.3, p.4))).collect();
    let limit = rejection_distance(k);
    let mut kept: Vec<usize> = (0..points.len()).collect();
    let mut rej = Vec::new();
    while kept.len() > min_n {
        let (mut worst, mut wd) = (usize::MAX, -1.0);
        for &i in &kept {
            let mut so = [[0.0; 2]; 2];
            let mut rhs = [0.0; 2];
            for &j in kept.iter().filter(|j| **j != i) {
                for a in 0..2 {
                    for b in 0..2 {
                        so[a][b] += om[j][a][b];
                    }
                    rhs[a] += om[j][a][0] * w[j].0 + om[j][a][1] * w[j].2;
                }
            }
            let cm = inv2(so);
            let mx = cm[0][0] * rhs[0] + cm[0][1] * rhs[1];
            let my = cm[1][0] * rhs[0] + cm[1][1] * rhs[1];
            let ci = cov(w[i].1, w[i].3, w[i].4);
            let mut tot = [[0.0; 2]; 2];
            for a in 0..2 {
                for b in 0..2 {
                    tot[a][b] = ci[a][b] + cm[a][b];
                }
            }
            let d = quad((w[i].0 - mx, w[i].2 - my), inv2(tot)).max(0.0).sqrt();
            if d > wd {
                worst = i;
                wd = d;
            }
        }
        if wd <= limit {
            break;
        }
        kept.retain(|x| *x != worst);
        rej.push(worst);
    }
    (kept, rej)
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub a: f64,
    pub b: f64,
    pub sa: f64,
    pub sb: f64,
    pub cov_ab: f64,
    pub mswd: f64,
    pub p: f64,
    pub n: usize,
    pub dof: i64,
}

fn horizontal_fit(pts: &[Pt]) -> Option<Line> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let w: Vec<f64> = pts.iter().map(|p| 1.0 / (p.3 / 2.0).powi(2)).collect();
    let sw: f64 = w.iter().sum();
    let a = w.iter().zip(pts).map(|(wi, p)| wi * p.2).sum::<f64>() / sw;
    let s: f64 = w.iter().zip(pts).map(|(wi, p)| wi * (p.2 - a).powi(2)).sum();
    Some(Line { a, b: 0.0, sa: (1.0 / sw).sqrt(), sb: 0.0, cov_ab: 0.0, mswd: s / (n - 1) as f64, p: chi2_sf(s, n as i64 - 1), n, dof: n as i64 - 1 })
}

pub fn york_fit(points: &[Pt], anchor: Option<(f64, f64)>) -> Option<Line> {
    let pts: Vec<Pt> = points.iter().copied().filter(|p| p.1 > 0.0 && p.3 > 0.0).collect();
    let n = pts.len();
    let dof = n as i64 - if anchor.is_some() { 1 } else { 2 };
    if dof < 1 {
        return None;
    }
    let wx: Vec<f64> = pts.iter().map(|p| 1.0 / (p.1 / 2.0).powi(2)).collect();
    let wy: Vec<f64> = pts.iter().map(|p| 1.0 / (p.3 / 2.0).powi(2)).collect();
    let r: Vec<f64> = pts.iter().map(|p| p.4).collect();
    let al: Vec<f64> = wx.iter().zip(&wy).map(|(a, b)| (a * b).sqrt()).collect();
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.2).collect();
    let mx = xs.iter().sum::<f64>() / n as f64;
    let my = ys.iter().sum::<f64>() / n as f64;
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let mut b = if sxx > 0.0 { xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / sxx } else { 0.0 };
    let weights = |b: f64| -> Vec<f64> { (0..n).map(|i| wx[i] * wy[i] / (wx[i] + b * b * wy[i] - 2.0 * b * r[i] * al[i])).collect() };
    let (mut xbar, mut ybar) = (0.0, 0.0);
    let mut beta = vec![0.0; n];
    for _ in 0..200 {
        let w = weights(b);
        if let Some(a) = anchor {
            xbar = a.0;
            ybar = a.1;
        } else {
            let sw: f64 = w.iter().sum();
            xbar = w.iter().zip(&xs).map(|(a, b)| a * b).sum::<f64>() / sw;
            ybar = w.iter().zip(&ys).map(|(a, b)| a * b).sum::<f64>() / sw;
        }
        let u: Vec<f64> = xs.iter().map(|x| x - xbar).collect();
        let v: Vec<f64> = ys.iter().map(|y| y - ybar).collect();
        beta = (0..n).map(|i| w[i] * (u[i] / wy[i] + b * v[i] / wx[i] - (b * u[i] + v[i]) * r[i] / al[i])).collect();
        let nb: f64 = (0..n).map(|i| w[i] * beta[i] * v[i]).sum();
        let db: f64 = (0..n).map(|i| w[i] * beta[i] * u[i]).sum();
        let bn = nb / db;
        if (bn - b).abs() <= 1e-15 * b.abs().max(1.0) {
            b = bn;
            break;
        }
        b = bn;
    }
    let w = weights(b);
    let a = ybar - b * xbar;
    let s: f64 = (0..n).map(|i| w[i] * (ys[i] - a - b * xs[i]).powi(2)).sum();
    let (sa, sb, cab) = if anchor.is_some() {
        let sb = (1.0 / (0..n).map(|i| w[i] * beta[i] * beta[i]).sum::<f64>()).sqrt();
        (xbar.abs() * sb, sb, -xbar * sb * sb)
    } else {
        let sw: f64 = w.iter().sum();
        let xadj: Vec<f64> = beta.iter().map(|bi| xbar + bi).collect();
        let xm = w.iter().zip(&xadj).map(|(a, b)| a * b).sum::<f64>() / sw;
        let sb = (1.0 / w.iter().zip(&xadj).map(|(wi, xa)| wi * (xa - xm).powi(2)).sum::<f64>()).sqrt();
        ((1.0 / sw + xm * xm * sb * sb).sqrt(), sb, -xm * sb * sb)
    };
    Some(Line { a, b, sa, sb, cov_ab: cab, mswd: s / dof as f64, p: chi2_sf(s, dof), n, dof })
}

#[derive(Clone, Debug, Default)]
pub struct Intercept {
    pub age_ma: f64,
    pub age_2s_ma: f64,
    pub age_2s_plain_ma: f64,
    pub anchored: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Discordia {
    pub fit: Line,
    pub intercepts: Vec<Intercept>,
    pub anchored: Option<f64>,
    pub expanded: bool,
}

/// York fit in Tera-Wasserburg space and its concordia intercepts; anchor
/// at a fixed age (0 Ma = horizontal line) when given.
pub fn discordia(points: &[Pt], c: &DecayConstants, anchor_age_ma: Option<f64>) -> Option<Discordia> {
    let (l8, l5) = (c.lambda_238, c.lambda_235);
    let mut anchored = false;
    let fit = match anchor_age_ma {
        Some(a) if a > 0.0 => {
            anchored = true;
            york_fit(points, Some(tw_point(a * 1e6, l8, l5)))
        }
        Some(_) => {
            anchored = true;
            let p: Vec<Pt> = points.iter().copied().filter(|p| p.1 > 0.0 && p.3 > 0.0).collect();
            horizontal_fit(&p)
        }
        None => york_fit(points, None),
    }?;
    let (a, b) = (fit.a, fit.b);
    let f = |t: f64| {
        let (x, y) = tw_point(t, l8, l5);
        a + b * x - y
    };
    let grid: Vec<f64> = (0..=2000).map(|k| 1e6 * (T_MAX_YEARS / 1e6).powf(k as f64 / 2000.0)).collect();
    let vals: Vec<f64> = grid.iter().map(|t| f(*t)).collect();
    let mut roots = Vec::new();
    for i in 0..2000 {
        let (f0, f1) = (vals[i], vals[i + 1]);
        if f0 == 0.0 || f0 * f1 < 0.0 {
            let (mut lo, mut hi) = (grid[i], grid[i + 1]);
            for _ in 0..100 {
                let mid = 0.5 * (lo + hi);
                if (f(mid) < 0.0) == (f(lo) < 0.0) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            roots.push(0.5 * (lo + hi));
        }
    }
    let (infl, _) = inflate_for_overdispersion(Some(fit.mswd), fit.dof);
    let expand = if infl { fit.mswd.sqrt() } else { 1.0 };
    let mut ints = Vec::new();
    for t in roots {
        let (x, _) = tw_point(t, l8, l5);
        let h = t * 1e-6;
        let ft = (f(t + h) - f(t - h)) / (2.0 * h);
        let (da, db) = (-1.0 / ft, -x / ft);
        let var = da * da * fit.sa * fit.sa + db * db * fit.sb * fit.sb + 2.0 * da * db * fit.cov_ab;
        let here = anchored && anchor_age_ma.is_some_and(|aa| (t / 1e6 - aa).abs() < 1e-4 * aa.max(1.0));
        let plain = if here { 0.0 } else { 2.0 * var.max(0.0).sqrt() / 1e6 };
        ints.push(Intercept { age_ma: t / 1e6, age_2s_plain_ma: plain, age_2s_ma: plain * expand, anchored: here });
    }
    if anchored && !ints.iter().any(|i| i.anchored) {
        ints.push(Intercept { age_ma: anchor_age_ma.unwrap(), age_2s_ma: 0.0, age_2s_plain_ma: 0.0, anchored: true });
    }
    ints.sort_by(|x, y| x.age_ma.partial_cmp(&y.age_ma).unwrap());
    Some(Discordia { fit, intercepts: ints, anchored: if anchored { anchor_age_ma } else { None }, expanded: expand > 1.0 })
}

/// Error ellipse points from 2-sigma errors; `scale` in 1-sigma units.
pub fn ellipse(x: f64, sx2: f64, y: f64, sy2: f64, rho: f64, scale: f64, n: usize) -> Vec<(f64, f64)> {
    let (sx, sy, r) = (sx2 / 2.0, sy2 / 2.0, rho.clamp(-0.999, 0.999));
    let (a, b, d) = (sx * sx, r * sx * sy, sy * sy);
    let (tr, det) = (a + d, a * d - b * b);
    let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
    let (l1, l2) = (tr / 2.0 + disc, (tr / 2.0 - disc).max(0.0));
    let th = 0.5 * (2.0 * b).atan2(a - d);
    let (w, h) = (scale * l1.sqrt(), scale * l2.sqrt());
    let (ct, st) = (th.cos(), th.sin());
    (0..=n)
        .map(|k| {
            let phi = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            let (u, v) = (w * phi.cos(), h * phi.sin());
            (x + u * ct - v * st, y + u * st + v * ct)
        })
        .collect()
}
