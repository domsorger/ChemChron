// SPDX-License-Identifier: GPL-3.0-or-later
//! Single-spot chemical Th-U-Pb age (total Pb model) and its uncertainty.

use crate::constants::DecayConstants;

pub fn total_radiogenic_pb_ppm(th_ppm: f64, u_ppm: f64, t: f64, c: &DecayConstants) -> f64 {
    let pb208 = th_ppm * ((c.lambda_232 * t).exp() - 1.0) * (208.0 / 232.0);
    let pb206 = u_ppm * c.u238_frac * ((c.lambda_238 * t).exp() - 1.0) * (206.0 / 238.0);
    let pb207 = u_ppm * c.u235_frac * ((c.lambda_235 * t).exp() - 1.0) * (207.0 / 235.0);
    pb208 + pb206 + pb207
}

/// Age in years; `None` when there is neither Th nor U.
pub fn solve_age_years(th_ppm: f64, u_ppm: f64, pb_ppm: f64, c: &DecayConstants, t_guess: f64) -> Option<f64> {
    if th_ppm <= 0.0 && u_ppm <= 0.0 {
        return None;
    }
    if pb_ppm <= 0.0 {
        return Some(0.0);
    }
    let residual = |t: f64| total_radiogenic_pb_ppm(th_ppm, u_ppm, t, c) - pb_ppm;
    let mut t = t_guess.max(1e6);
    for _ in 0..100 {
        let y = residual(t);
        let dt = t * 1e-6 + 1.0;
        let dy = (residual(t + dt) - y) / dt;
        if dy == 0.0 {
            break;
        }
        let mut new_t = t - y / dy;
        if new_t <= 0.0 {
            new_t = t / 2.0;
        }
        if (new_t - t).abs() < 1.0 {
            t = new_t;
            break;
        }
        t = new_t;
    }
    let (mut lo, mut hi) = (0.0f64, 5.0e10f64);
    if residual(lo) * residual(hi) < 0.0 {
        for _ in 0..200 {
            let mid = (lo + hi) / 2.0;
            if residual(lo) * residual(mid) <= 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
            if hi - lo < 1.0 {
                break;
            }
        }
        let bisect_t = (lo + hi) / 2.0;
        if residual(bisect_t).abs() < residual(t).abs() || t.is_nan() || t < 0.0 {
            t = bisect_t;
        }
    }
    Some(t.max(0.0))
}

/// 2-sigma age uncertainty in years, by linear propagation of the Th, U and Pb errors.
#[allow(clippy::too_many_arguments)]
pub fn propagate_age_error(
    th_ppm: f64,
    u_ppm: f64,
    _pb_ppm: f64,
    s_th: f64,
    s_u: f64,
    s_pb: f64,
    c: &DecayConstants,
    t: Option<f64>,
) -> f64 {
    let Some(t) = t else { return 0.0 };
    if t <= 0.0 {
        return 0.0;
    }
    let (a232, a238, a235) = (208.0 / 232.0, 206.0 / 238.0, 207.0 / 235.0);
    let d_th = a232 * ((c.lambda_232 * t).exp() - 1.0);
    let d_u = c.u238_frac * a238 * ((c.lambda_238 * t).exp() - 1.0) + c.u235_frac * a235 * ((c.lambda_235 * t).exp() - 1.0);
    let d_t = th_ppm * a232 * c.lambda_232 * (c.lambda_232 * t).exp()
        + u_ppm
            * (c.u238_frac * a238 * c.lambda_238 * (c.lambda_238 * t).exp()
                + c.u235_frac * a235 * c.lambda_235 * (c.lambda_235 * t).exp());
    if d_t == 0.0 {
        return 0.0;
    }
    let var = (-d_th / d_t * s_th).powi(2) + (-d_u / d_t * s_u).powi(2) + (1.0 / d_t * s_pb).powi(2);
    2.0 * var.max(0.0).sqrt()
}
