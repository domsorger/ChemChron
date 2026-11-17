// SPDX-License-Identifier: GPL-3.0-or-later
//! Statistics over sweeps and analyses: means with flagged outliers, weighted
//! means, MSWD and excess uncertainty.

use serde::{Deserialize, Serialize};

/// Mean of the sweeps in one selection. Outliers are counted, never hidden.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeanSe {
    pub mean: f64,
    /// Two standard errors of the mean over the retained sweeps.
    pub se2: f64,
    pub sd: f64,
    /// Sweeps used in the mean.
    pub n: usize,
    /// Sweeps with a finite value that the outlier rule rejected.
    pub n_outliers: usize,
}

pub fn median(values: &[f64]) -> f64 {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 0 {
        0.5 * (v[m - 1] + v[m])
    } else {
        v[m]
    }
}

/// Outlier rule: reject sweeps further than `k` robust standard deviations
/// (1.4826 x median absolute deviation) from the median, repeated until stable
/// (at most 3 passes). When the robust spread is zero the ordinary standard
/// deviation is used. `k <= 0` turns rejection off. Returns one keep-flag per value.
pub fn outlier_mask(values: &[f64], k: f64) -> Vec<bool> {
    let mut keep: Vec<bool> = values.iter().map(|v| v.is_finite()).collect();
    if k <= 0.0 {
        return keep;
    }
    for _ in 0..3 {
        let kept: Vec<f64> = values
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(v, _)| *v)
            .collect();
        if kept.len() < 4 {
            break;
        }
        let med = median(&kept);
        let dev: Vec<f64> = kept.iter().map(|v| (v - med).abs()).collect();
        let mut spread = 1.4826 * median(&dev);
        if spread <= 0.0 {
            let m = kept.iter().sum::<f64>() / kept.len() as f64;
            spread = (kept.iter().map(|v| (v - m).powi(2)).sum::<f64>()
                / (kept.len() - 1) as f64)
                .sqrt();
        }
        if spread <= 0.0 {
            break;
        }
        let mut changed = false;
        for (i, v) in values.iter().enumerate() {
            if keep[i] && (v - med).abs() > k * spread {
                keep[i] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    keep
}

pub fn mean_se_masked(values: &[f64], keep: &[bool]) -> MeanSe {
    let mut n = 0usize;
    let mut sum = 0.0;
    let mut finite = 0usize;
    for (v, k) in values.iter().zip(keep) {
        if v.is_finite() {
            finite += 1;
            if *k {
                n += 1;
                sum += v;
            }
        }
    }
    if n == 0 {
        return MeanSe {
            mean: f64::NAN,
            se2: f64::NAN,
            sd: f64::NAN,
            n: 0,
            n_outliers: finite,
        };
    }
    let mean = sum / n as f64;
    let mut ss = 0.0;
    for (v, k) in values.iter().zip(keep) {
        if v.is_finite() && *k {
            ss += (v - mean).powi(2);
        }
    }
    let sd = if n > 1 { (ss / (n - 1) as f64).sqrt() } else { f64::NAN };
    MeanSe {
        mean,
        se2: if n > 1 { 2.0 * sd / (n as f64).sqrt() } else { f64::NAN },
        sd,
        n,
        n_outliers: finite - n,
    }
}

pub fn mean_se(values: &[f64], k: f64) -> MeanSe {
    let keep = outlier_mask(values, k);
    mean_se_masked(values, &keep)
}

/// Correlation of two sweep series over the sweeps both keep.
pub fn correlation(a: &[f64], b: &[f64], keep: &[bool]) -> f64 {
    let mut n = 0.0;
    let (mut sa, mut sb) = (0.0, 0.0);
    for i in 0..a.len() {
        if keep[i] && a[i].is_finite() && b[i].is_finite() {
            n += 1.0;
            sa += a[i];
            sb += b[i];
        }
    }
    if n < 3.0 {
        return 0.0;
    }
    let (ma, mb) = (sa / n, sb / n);
    let (mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        if keep[i] && a[i].is_finite() && b[i].is_finite() {
            saa += (a[i] - ma).powi(2);
            sbb += (b[i] - mb).powi(2);
            sab += (a[i] - ma) * (b[i] - mb);
        }
    }
    if saa <= 0.0 || sbb <= 0.0 {
        return 0.0;
    }
    (sab / (saa * sbb).sqrt()).clamp(-0.999, 0.999)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WeightedMean {
    pub mean: f64,
    /// Two sigma, from the input uncertainties alone.
    pub unc2s: f64,
    /// Two sigma multiplied by sqrt(MSWD) when MSWD exceeds 1.
    pub unc2s_mswd: f64,
    pub mswd: f64,
    pub p: f64,
    pub n: usize,
}

/// Weighted mean of analyses with 2 sigma uncertainties.
pub fn weighted_mean(values: &[(f64, f64)]) -> Option<WeightedMean> {
    let pts: Vec<(f64, f64)> = values
        .iter()
        .copied()
        .filter(|(v, s)| v.is_finite() && s.is_finite() && *s > 0.0)
        .collect();
    if pts.is_empty() {
        return None;
    }
    let mut sw = 0.0;
    let mut swx = 0.0;
    for (v, s2) in &pts {
        let w = 4.0 / (s2 * s2);
        sw += w;
        swx += w * v;
    }
    let mean = swx / sw;
    let chi: f64 = pts
        .iter()
        .map(|(v, s2)| (v - mean).powi(2) * 4.0 / (s2 * s2))
        .sum();
    let dof = pts.len().saturating_sub(1);
    let mswd = if dof > 0 { chi / dof as f64 } else { 0.0 };
    let p = if dof > 0 { chi2_sf(chi, dof as f64) } else { 1.0 };
    let unc2s = 2.0 / sw.sqrt();
    Some(WeightedMean {
        mean,
        unc2s,
        // enlarged by √MSWD only when the scatter is significant (P < 0.05),
        // as in ChemChron's Combined results
        unc2s_mswd: if dof > 0 && p < 0.05 { unc2s * mswd.sqrt().max(1.0) } else { unc2s },
        mswd,
        p,
        n: pts.len(),
    })
}

/// Probability that chi-squared with `dof` degrees of freedom exceeds `x`.
pub fn chi2_sf(x: f64, dof: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    1.0 - gamma_p(dof / 2.0, x / 2.0)
}

fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 6] = [
        76.18009172947146,
        -86.50532032941677,
        24.01409824083091,
        -1.231739572450155,
        0.1208650973866179e-2,
        -0.5395239384953e-5,
    ];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for c in C {
        y += 1.0;
        ser += c / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

fn gamma_p(a: f64, x: f64) -> f64 {
    if x < a + 1.0 {
        let mut ap = a;
        let mut del = 1.0 / a;
        let mut sum = del;
        for _ in 0..500 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-14 {
                break;
            }
        }
        sum * (-x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / 1e-300;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..500 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < 1e-300 {
                d = 1e-300;
            }
            c = b + an / c;
            if c.abs() < 1e-300 {
                c = 1e-300;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-14 {
                break;
            }
        }
        1.0 - (-x + a * x.ln() - ln_gamma(a)).exp() * h
    }
}

/// Relative excess uncertainty (fraction, 1 sigma) that brings the MSWD of
/// jack-knifed residuals to 1. `residuals` are relative to the fit, `sigmas`
/// are the matching relative 1 sigma internal uncertainties.
pub fn excess_for_unit_mswd(residuals: &[f64], sigmas: &[f64]) -> f64 {
    let pairs: Vec<(f64, f64)> = residuals
        .iter()
        .zip(sigmas)
        .filter(|(r, s)| r.is_finite() && s.is_finite() && **s > 0.0)
        .map(|(r, s)| (*r, *s))
        .collect();
    if pairs.len() < 3 {
        return 0.0;
    }
    let dof = (pairs.len() - 1) as f64;
    let mswd = |e: f64| -> f64 {
        pairs
            .iter()
            .map(|(r, s)| r * r / (s * s + e * e))
            .sum::<f64>()
            / dof
    };
    if mswd(0.0) <= 1.0 {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    while mswd(hi) > 1.0 && hi < 1e6 {
        hi *= 2.0;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if mswd(mid) > 1.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Relative excess (1 sigma) that brings the pooled MSWD of the primary's
/// residuals (relative to its normalisation curve) and of the secondary
/// reference materials (each scattered about its own weighted mean) to 1.
/// `primary`: (relative residual, relative sigma); `groups`: per secondary
/// material, (relative value, relative sigma) of its analyses.
pub fn pooled_excess(primary: &[(f64, f64)], groups: &[Vec<(f64, f64)>]) -> f64 {
    let ok = |p: &&(f64, f64)| p.0.is_finite() && p.1.is_finite() && p.1 > 0.0;
    let prim: Vec<(f64, f64)> = primary.iter().filter(ok).copied().collect();
    let groups: Vec<Vec<(f64, f64)>> = groups.iter().map(|g| g.iter().filter(ok).copied().collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
    let dof = prim.len().saturating_sub(1) + groups.iter().map(|g| g.len() - 1).sum::<usize>();
    if dof < 2 {
        return 0.0;
    }
    let sum = |e: f64| -> f64 {
        let mut s: f64 = prim.iter().map(|(r, sg)| r * r / (sg * sg + e * e)).sum();
        for g in &groups {
            let w: Vec<f64> = g.iter().map(|(_, sg)| 1.0 / (sg * sg + e * e)).collect();
            let m = g.iter().zip(&w).map(|((x, _), w)| x * w).sum::<f64>() / w.iter().sum::<f64>();
            s += g.iter().zip(&w).map(|((x, _), w)| (x - m).powi(2) * w).sum::<f64>();
        }
        s / dof as f64
    };
    if sum(0.0) <= 1.0 {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, 1e-3);
    while sum(hi) > 1.0 && hi < 10.0 {
        hi *= 2.0;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if sum(mid) > 1.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondaries_raise_the_excess_when_they_scatter_more() {
        let prim: Vec<(f64, f64)> = (0..20).map(|i| (if i % 2 == 0 { 0.004 } else { -0.004 }, 0.004)).collect();
        let alone = pooled_excess(&prim, &[]);
        let sec: Vec<(f64, f64)> = (0..12).map(|i| (1.0 + if i % 2 == 0 { 0.02 } else { -0.02 }, 0.004)).collect();
        let both = pooled_excess(&prim, &[sec]);
        assert!(alone < 0.001, "{alone}");
        assert!(both > 0.008, "{both}");
    }

    #[test]
    fn spike_is_rejected_and_counted() {
        let mut v: Vec<f64> = (0..50).map(|i| 100.0 + (i % 5) as f64).collect();
        v[10] = 1e6;
        let r = mean_se(&v, 3.0);
        assert_eq!(r.n_outliers, 1);
        assert_eq!(r.n, 49);
        assert!((r.mean - 102.0).abs() < 0.2);
    }

    #[test]
    fn weighted_mean_of_equal_values_has_zero_mswd() {
        let w = weighted_mean(&[(10.0, 1.0), (10.0, 1.0), (10.0, 2.0)]).unwrap();
        assert!((w.mean - 10.0).abs() < 1e-12);
        assert!(w.mswd.abs() < 1e-12);
        assert!((w.p - 1.0).abs() < 1e-9);
    }

    #[test]
    fn chi_squared_matches_tabulated_values() {
        assert!((chi2_sf(3.841, 1.0) - 0.05).abs() < 1e-3);
        assert!((chi2_sf(18.307, 10.0) - 0.05).abs() < 1e-3);
    }

    #[test]
    fn excess_brings_mswd_to_one() {
        let r = [0.02, -0.02, 0.018, -0.022, 0.02, -0.018];
        let s = [0.005; 6];
        let e = excess_for_unit_mswd(&r, &s);
        let m: f64 = r
            .iter()
            .zip(&s)
            .map(|(r, s)| r * r / (s * s + e * e))
            .sum::<f64>()
            / 5.0;
        assert!((m - 1.0).abs() < 1e-6);
    }
}
