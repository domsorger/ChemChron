// SPDX-License-Identifier: GPL-3.0-or-later
//! Weighted means, MSWD, overdispersion, outlier rejection and group checks.

use crate::constants::{DecimalPlaces, DEFAULT_MINERAL};
use crate::reference::{OUTSIDE_2S, WITHIN_2S};
use crate::row::*;
use std::collections::BTreeMap;

pub const MIN_N_FOR_LONG_TERM_REL_PCT: usize = 3;
pub const MIN_N_FOR_GROUP_OUTLIER_CHECK: usize = 3;
pub const MIN_N_FOR_WEIGHTED_MEAN: usize = 2;
pub const DEFAULT_OUTLIER_REJECTION_SIGMA: f64 = 2.0;
pub const REJECTION_WARNING_FRACTION: f64 = 0.20;
pub const OVERDISPERSION_ALPHA: f64 = 0.05;

pub fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation (g = 7, n = 9), accurate to ~1e-15.
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).abs().ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// P(chi-square with k degrees of freedom > x).
pub fn chi2_sf(x: f64, k: i64) -> f64 {
    if k <= 0 || x.is_nan() {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 1.0;
    }
    let (a, z) = (0.5 * k as f64, 0.5 * x);
    let gln = ln_gamma(a);
    if z < a + 1.0 {
        let mut term = 1.0 / a;
        let mut total = term;
        let mut ap = a;
        for _ in 0..500 {
            ap += 1.0;
            term *= z / ap;
            total += term;
            if term.abs() < total.abs() * 1e-15 {
                break;
            }
        }
        return (1.0 - total * (-z + a * z.ln() - gln).exp()).max(0.0);
    }
    let mut b = z + 1.0 - a;
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
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-15 {
            break;
        }
    }
    (-z + a * z.ln() - gln).exp() * h
}

/// (inflate, p): expand by sqrt(MSWD) when MSWD > 1 and P < 0.05 (IsoplotR's rule).
pub fn inflate_for_overdispersion(mswd: Option<f64>, dof: i64) -> (bool, f64) {
    let Some(m) = mswd else { return (false, f64::NAN) };
    if dof <= 0 || m.is_nan() {
        return (false, f64::NAN);
    }
    let p = chi2_sf(m * dof as f64, dof);
    (m > 1.0 && p < OVERDISPERSION_ALPHA, p)
}

pub fn pbo_below_detection_limit(row: &Row) -> bool {
    text(row, "Below_DL").split(',').any(|p| p.trim() == "PbO")
}

pub fn usable_for_age_statistics(row: &Row) -> bool {
    flag(row, "Include_In_Average", true) && !pbo_below_detection_limit(row)
}

pub fn rejection_warning(n_rejected: usize, n_total: usize) -> String {
    if n_total == 0 || (n_rejected as f64 / n_total as f64) <= REJECTION_WARNING_FRACTION {
        return String::new();
    }
    format!(
        "Warning: {:.0} % of analyses rejected -- likely more than one age population, not just outliers",
        100.0 * n_rejected as f64 / n_total as f64
    )
}

#[derive(Clone, Debug, Default)]
pub struct WeightedMean {
    pub mean: f64,
    pub internal_2s: f64,
    pub expanded_2s: f64,
    pub mswd: f64,
    pub mswd_lower: f64,
    pub mswd_upper: f64,
    pub p_value: f64,
    pub inflated: bool,
    pub n: usize,
    /// (age, sigma 1s) rejected, in rejection order.
    pub rejected: Vec<(f64, f64)>,
    /// Indices into the input of the rejected analyses.
    pub rejected_idx: Vec<usize>,
    /// Indices into the input of the analyses used.
    pub used_idx: Vec<usize>,
    pub long_term_2s: f64,
    pub total_2s: f64,
    pub total_2s_plain: f64,
}

fn solve(pairs: &[(f64, f64)]) -> WeightedMean {
    let n = pairs.len();
    let w: Vec<f64> = pairs.iter().map(|(_, s)| 1.0 / (s * s)).collect();
    let sw: f64 = w.iter().sum();
    let mean = pairs.iter().zip(&w).map(|((a, _), wi)| wi * a).sum::<f64>() / sw;
    let internal = 2.0 / sw.sqrt();
    let dof = n - 1;
    let mswd = pairs.iter().zip(&w).map(|((a, _), wi)| wi * (a - mean).powi(2)).sum::<f64>() / dof as f64;
    let half = 2.0 * (2.0 / dof as f64).sqrt();
    let (inflated, p) = inflate_for_overdispersion(Some(mswd), dof as i64);
    WeightedMean {
        mean,
        internal_2s: internal,
        expanded_2s: if inflated { internal * mswd.sqrt() } else { internal },
        mswd,
        mswd_lower: (1.0 - half).max(0.0),
        mswd_upper: 1.0 + half,
        p_value: p,
        inflated,
        n,
        ..Default::default()
    }
}

/// Leave-one-out iterative rejection over (index, age, sigma) triples.
pub fn iteratively_reject(mut t: Vec<(usize, f64, f64)>, k: f64) -> (Vec<(usize, f64, f64)>, Vec<(usize, f64, f64)>) {
    let mut rejected = Vec::new();
    while t.len() > MIN_N_FOR_WEIGHTED_MEAN {
        let w: Vec<f64> = t.iter().map(|(_, _, s)| 1.0 / (s * s)).collect();
        let sw: f64 = w.iter().sum();
        let swa: f64 = w.iter().zip(&t).map(|(wi, (_, a, _))| wi * a).sum();
        let mut worst = 0;
        let mut worst_r = f64::NEG_INFINITY;
        for (i, (wi, (_, a, s))) in w.iter().zip(&t).enumerate() {
            let ow = sw - wi;
            let mo = (swa - wi * a) / ow;
            let r = (a - mo).abs() / (s * s + 1.0 / ow).sqrt();
            if r > worst_r {
                worst_r = r;
                worst = i;
            }
        }
        if worst_r <= k {
            break;
        }
        rejected.push(t.remove(worst));
    }
    (t, rejected)
}

/// Weighted mean of bare (age, sigma 1s) pairs.
pub fn weighted_mean(ages: &[f64], s1: &[f64], reject: bool, k: f64) -> Option<WeightedMean> {
    let mut t: Vec<(usize, f64, f64)> = Vec::new();
    for (i, (a, s)) in ages.iter().zip(s1).enumerate() {
        if a.is_finite() && s.is_finite() && *s > 0.0 {
            t.push((i, *a, *s));
        }
    }
    if t.len() < MIN_N_FOR_WEIGHTED_MEAN {
        return None;
    }
    let mut rej = Vec::new();
    if reject {
        let (kept, r) = iteratively_reject(t, k);
        t = kept;
        rej = r;
    }
    let pairs: Vec<(f64, f64)> = t.iter().map(|(_, a, s)| (*a, *s)).collect();
    let mut r = solve(&pairs);
    r.rejected = rej.iter().map(|(_, a, s)| (*a, *s)).collect();
    r.rejected_idx = rej.iter().map(|x| x.0).collect();
    r.used_idx = t.iter().map(|x| x.0).collect();
    r.long_term_2s = 0.0;
    r.total_2s = r.expanded_2s;
    r.total_2s_plain = r.internal_2s;
    Some(r)
}

/// (Age_Ma, 1-sigma point uncertainty) of a row.
pub fn row_age_sigma(row: &Row) -> Option<(f64, f64)> {
    let age = num(row, "Age_Ma")?;
    let s2 = num(row, "Age_2s_Ma_point").or_else(|| num(row, "Age_2s_Ma"))?;
    Some((age, s2 / 2.0))
}

pub fn long_term_rel_2s_pct(rows: &[&Row]) -> f64 {
    let mut v = Vec::new();
    for row in rows {
        if let Some(x) = num(row, "LongTerm_2s_rel_pct") {
            v.push(x);
            continue;
        }
        if row.contains_key("LongTerm_2s_rel_pct") {
            continue;
        }
        if let (Some(age), Some(total), Some(an)) = (num(row, "Age_Ma"), num(row, "Age_2s_Ma_total"), num(row, "Age_2s_Ma")) {
            if age > 0.0 && total >= an {
                v.push(100.0 * (total * total - an * an).sqrt() / age);
            }
        }
    }
    if v.is_empty() {
        0.0
    } else {
        mean(&v)
    }
}

/// Weighted mean of result rows (point uncertainties as weights, the
/// systematic term added once). Indices refer to `rows`.
pub fn weighted_mean_for_rows(rows: &[&Row], reject: bool, k: f64) -> Option<WeightedMean> {
    let mut t: Vec<(usize, f64, f64)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if pbo_below_detection_limit(row) {
            continue;
        }
        if let Some((a, s)) = row_age_sigma(row) {
            t.push((i, a, s));
        }
    }
    if t.is_empty() {
        return None;
    }
    let mut rej = Vec::new();
    if reject {
        let (kept, r) = iteratively_reject(t, k);
        t = kept;
        rej = r;
    }
    if t.len() < MIN_N_FOR_WEIGHTED_MEAN {
        return None;
    }
    let pairs: Vec<(f64, f64)> = t.iter().map(|(_, a, s)| (*a, *s)).collect();
    let mut r = solve(&pairs);
    r.rejected = rej.iter().map(|(_, a, s)| (*a, *s)).collect();
    r.rejected_idx = rej.iter().map(|x| x.0).collect();
    r.used_idx = t.iter().map(|x| x.0).collect();
    let used: Vec<&Row> = t.iter().map(|x| rows[x.0]).collect();
    let lt = long_term_rel_2s_pct(&used) / 100.0 * r.mean;
    r.long_term_2s = lt;
    r.total_2s = (r.expanded_2s.powi(2) + lt * lt).sqrt();
    r.total_2s_plain = (r.internal_2s.powi(2) + lt * lt).sqrt();
    Some(r)
}

/// Per reference material: long-term mean, scatter and RMS deviation from accepted.
pub fn long_term_overview(log_rows: &[Row], refs: &crate::reference::Refs, dp: &DecimalPlaces) -> Vec<Row> {
    let mut groups: Vec<(String, Vec<f64>)> = Vec::new();
    for row in log_rows {
        let code = text(row, "Reference");
        let Some(age) = num(row, "Age_Ma") else { continue };
        if code.is_empty() {
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == code) {
            Some(g) => g.1.push(age),
            None => groups.push((code, vec![age])),
        }
    }
    let mut out = Vec::new();
    for (code, ages) in groups {
        let m = mean(&ages);
        let sd2 = if ages.len() > 1 { 2.0 * pstdev(&ages) } else { 0.0 };
        let accepted = crate::reference::ref_num(refs, &code, "age");
        let mut r = Row::new();
        set(&mut r, "Reference", jstr(&code));
        set(&mut r, "N_logged", jnum(ages.len() as f64));
        set(&mut r, "LongTerm_Mean_Age_Ma", jround(m, dp.age));
        set(&mut r, "LongTerm_2s_Ma", jround(sd2, dp.age));
        set(&mut r, "Accepted_Age_Ma", jopt(accepted));
        set(&mut r, "LongTerm_Diff_Ma", accepted.map(|a| jround(m - a, dp.age)).unwrap_or_else(blank));
        let rel = match accepted {
            Some(a) if a > 0.0 => {
                let rms = (ages.iter().map(|x| ((x - a) / a).powi(2)).sum::<f64>() / ages.len() as f64).sqrt();
                jround(100.0 * rms, dp.age)
            }
            _ => blank(),
        };
        set(&mut r, "LongTerm_RelDev_pct", rel);
        out.push(r);
    }
    out
}

/// Pooled long-term relative uncertainty over materials with N >= min_n.
pub fn pooled_long_term_rel_pct(overview: &[Row], min_n: usize) -> (Option<f64>, Vec<(String, f64, usize)>) {
    let mut v = Vec::new();
    let mut contributing = Vec::new();
    for r in overview {
        let n = num(r, "N_logged").unwrap_or(0.0) as usize;
        if n < min_n {
            continue;
        }
        let Some(rel) = num(r, "LongTerm_RelDev_pct") else { continue };
        v.push(rel);
        contributing.push((text(r, "Reference"), round_to(rel, 2), n));
    }
    if v.is_empty() {
        (None, Vec::new())
    } else {
        (Some(mean(&v)), contributing)
    }
}

pub fn mineral_of(row: &Row) -> String {
    let m = text(row, "Mineral");
    if m.is_empty() {
        DEFAULT_MINERAL.to_string()
    } else {
        m
    }
}

/// Group check of sample rows against their own (Flag, Mineral) group.
pub fn annotate_group_outliers(rows: &mut [Row], dp: &DecimalPlaces) {
    let mut groups: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    for row in rows.iter() {
        if truthy(row, "Is_Reference") || !usable_for_age_statistics(row) {
            continue;
        }
        let flag = text(row, "Flag");
        let Some(age) = num(row, "Age_Ma") else { continue };
        if flag.is_empty() {
            continue;
        }
        groups.entry((flag, mineral_of(row))).or_default().push(age);
    }
    let stats: BTreeMap<(String, String), (f64, f64)> = groups
        .into_iter()
        .filter(|(_, a)| a.len() >= MIN_N_FOR_GROUP_OUTLIER_CHECK)
        .map(|(k, a)| (k, (mean(&a), 2.0 * pstdev(&a))))
        .collect();
    for row in rows.iter_mut() {
        if truthy(row, "Is_Reference") {
            continue;
        }
        let flag = text(row, "Flag");
        let key = (flag.clone(), mineral_of(row));
        let gs = if flag.is_empty() { None } else { stats.get(&key) };
        let age = num(row, "Age_Ma");
        let (Some((m, sd2)), Some(age)) = (gs, age) else {
            set(row, "Group_Diff_Ma", blank());
            set(row, "Group_Diff_pct", blank());
            set(row, "Group_Within_2s", blank());
            continue;
        };
        let diff = age - m;
        let own = num(row, "Age_2s_Ma_point").or_else(|| num(row, "Age_2s_Ma")).unwrap_or(0.0);
        let comb = (own * own + sd2 * sd2).sqrt();
        set(row, "Group_Diff_Ma", jround(diff, dp.age));
        set(row, "Group_Diff_pct", if *m != 0.0 { jround(100.0 * diff / m, dp.age) } else { blank() });
        set(row, "Group_Within_2s", jstr(if diff.abs() <= comb { WITHIN_2S } else { OUTSIDE_2S }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chi2_known_values() {
        // P(chi2_1 > 3.841) = 0.05; P(chi2_10 > 18.307) = 0.05
        assert!((chi2_sf(3.841_458_8, 1) - 0.05).abs() < 1e-6);
        assert!((chi2_sf(18.307_038, 10) - 0.05).abs() < 1e-6);
        assert!((ln_gamma(5.0) - 24f64.ln()).abs() < 1e-12);
    }
}
