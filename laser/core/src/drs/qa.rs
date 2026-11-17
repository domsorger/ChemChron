// SPDX-License-Identifier: GPL-3.0-or-later
//! Quality checks: automatic flags for each analysis, and comparison of every
//! reference material with its accepted values.

use super::upb::RatioKind;
use super::Reduction;
use crate::geochron::{concordia_age, ConcordiaAge, ConcordiaPoint};
use crate::model::*;
use crate::stats::{self, WeightedMean};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum QaStatus {
    Pass,
    Warn,
    Alarm,
    /// Used for normalising, so it agrees by construction: not independent evidence.
    Primary,
}

#[derive(Clone, Debug, Serialize)]
pub struct QaRow {
    pub group: String,
    pub material: String,
    pub quantity: String,
    pub measured: f64,
    pub measured_2s: f64,
    pub accepted: f64,
    pub accepted_2s: f64,
    pub diff_pct: f64,
    pub mswd: f64,
    pub n: usize,
    pub status: QaStatus,
}

#[derive(Clone, Debug, Default)]
pub struct GroupStats {
    pub group: u64,
    pub name: String,
    pub n: usize,
    pub concordia: Option<ConcordiaAge>,
    pub wm_r68: Option<WeightedMean>,
    pub wm_a68: Option<WeightedMean>,
    pub wm_a76: Option<WeightedMean>,
    pub wm_a82: Option<WeightedMean>,
    pub wm_a68_common: Option<WeightedMean>,
    pub wm_a68_th230: Option<WeightedMean>,
}

#[derive(Clone, Debug, Default)]
pub struct QaReport {
    pub rows: Vec<QaRow>,
    pub groups: Vec<GroupStats>,
}

impl QaReport {
    pub fn alarms(&self) -> usize {
        self.rows.iter().filter(|r| r.status == QaStatus::Alarm).count()
    }
    pub fn warnings(&self) -> usize {
        self.rows.iter().filter(|r| r.status == QaStatus::Warn).count()
    }
}

/// Flags in plain words, one reason each.
pub fn flag_selections(session: &Session, red: &mut Reduction) {
    let qa = &session.settings.qa;
    for (s, result) in session.selections.iter().zip(red.selections.iter_mut()) {
        let Some(group) = session.group_of(s) else { continue };
        if group.kind == GroupKind::Baseline || s.excluded {
            continue;
        }
        let dt = session.inputs.sweep_seconds();
        if (result.range.len() as f64) * dt < 5.0 {
            result.flags.push("Short selection".to_string());
        }
        // Glass carries ordinary lead, so isotope checks say nothing about it.
        if session.rm_of(group.id).is_some_and(|m| m.matrix == "Glass") {
            continue;
        }
        if let Some(u) = &result.upb {
            let weak = !(u.r68.mean > 0.0)
                || !(u.r68.int2se / u.r68.mean < 0.25)
                || !(u.r76.mean > 0.0)
                || !u.a76.prop2s.is_finite();
            if weak {
                result.flags.push("Weak signal: ratios unreliable".to_string());
            }
            if u.a68.age > 4600.0 || u.a76.age > 4600.0 {
                result.flags.push("Age above 4600 Ma".to_string());
            }
            if u.discordance_pct.abs() > qa.flag_discordance_pct {
                result.flags.push(format!("Discordant {:.0} %", u.discordance_pct));
            }
            // Only when it stands clear of the counting noise on mass 204.
            if u.f206_pct_204.is_finite()
                && u.f206_pct_204 > qa.flag_common_pct
                && u.f206_pct_204 > 2.0 * u.f206_pct_204_2s
            {
                result.flags.push(format!("Common Pb {:.1} %", u.f206_pct_204));
            }
            let total = u.r68.n + u.r68.n_outliers;
            if total > 0 && 100.0 * u.r68.n_outliers as f64 / total as f64 > qa.flag_outlier_pct {
                result.flags.push(format!("Many outlier sweeps ({}/{total})", u.r68.n_outliers));
            }
            if u.trend_sigma.abs() > 5.0 {
                result.flags.push("Ratio changes during ablation".to_string());
            }
        }
    }
}

fn status_for(primary: bool, diff: f64, warn: f64, alarm: f64) -> QaStatus {
    if primary {
        QaStatus::Primary
    } else if !diff.is_finite() || diff.abs() >= alarm {
        QaStatus::Alarm
    } else if diff.abs() >= warn {
        QaStatus::Warn
    } else {
        QaStatus::Pass
    }
}

pub fn report(session: &Session, red: &Reduction) -> QaReport {
    let mut out = QaReport::default();
    let qa = &session.settings.qa;
    let d = &session.settings.upb.decay;
    for g in &session.groups {
        if g.kind == GroupKind::Baseline {
            continue;
        }
        let rows: Vec<(&Selection, &super::SelResult)> = session
            .selections
            .iter()
            .zip(&red.selections)
            .filter(|(s, _)| s.group == g.id && !s.excluded)
            .collect();
        // Calibration glass carries ordinary lead: its isotope ratios are not ages.
        let glass = session.rm_of(g.id).is_some_and(|m| m.matrix == "Glass");
        let upb: Vec<&super::upb::UpbValue> = rows
            .iter()
            .filter(|_| !glass)
            .filter_map(|(_, r)| r.upb.as_ref())
            .collect();
        let mut stats_row = GroupStats {
            group: g.id,
            name: g.name.clone(),
            n: rows.len(),
            ..Default::default()
        };
        if !upb.is_empty() {
            let points: Vec<ConcordiaPoint> = upb
                .iter()
                .map(|u| ConcordiaPoint {
                    x: u.r75.mean,
                    y: u.r68.mean,
                    sx: u.r75.prop2s,
                    sy: u.r68.prop2s,
                    rho: u.rho_68_75,
                })
                .collect();
            stats_row.concordia = concordia_age(&points, d);
            let wm = |f: &dyn Fn(&super::upb::UpbValue) -> (f64, f64)| {
                let v: Vec<(f64, f64)> = upb.iter().map(|u| f(u)).collect();
                stats::weighted_mean(&v)
            };
            stats_row.wm_r68 = wm(&|u| (u.r68.mean, u.r68.prop2s));
            stats_row.wm_a68 = wm(&|u| (u.a68.age, u.a68.prop2s));
            stats_row.wm_a76 = wm(&|u| (u.a76.age, u.a76.prop2s));
            stats_row.wm_a82 = wm(&|u| u.a82.map_or((f64::NAN, f64::NAN), |a| (a.age, a.prop2s)));
            stats_row.wm_a68_common = wm(&|u| {
                u.common.as_ref().map_or((f64::NAN, f64::NAN), |c| (c.age68, c.age68_2s))
            });
            stats_row.wm_a68_th230 = wm(&|u| u.a68_th230.map_or((f64::NAN, f64::NAN), |a| (a.age, a.prop2s)));
        }
        let Some(rm) = session.rm_of(g.id) else {
            out.groups.push(stats_row);
            continue;
        };
        // Isotope ratios.
        let primary_upb = session.settings.upb.primary_group == Some(g.id);
        for kind in RatioKind::ALL {
            let Some(accepted) = rm.get(kind.key()) else { continue };
            let values: Vec<(f64, f64)> = upb
                .iter()
                .filter_map(|u| match kind {
                    RatioKind::R68 => Some((u.r68.mean, u.r68.prop2s)),
                    RatioKind::R75 => Some((u.r75.mean, u.r75.prop2s)),
                    RatioKind::R76 => Some((u.r76.mean, u.r76.prop2s)),
                    RatioKind::R82 => u.r82.as_ref().map(|r| (r.mean, r.prop2s)),
                })
                .collect();
            if let Some(w) = stats::weighted_mean(&values) {
                let diff = 100.0 * (w.mean / accepted.value - 1.0);
                out.rows.push(QaRow {
                    group: g.name.clone(),
                    material: rm.name.clone(),
                    quantity: kind.key().to_string(),
                    measured: w.mean,
                    measured_2s: w.unc2s_mswd,
                    accepted: accepted.value,
                    accepted_2s: accepted.unc2s,
                    diff_pct: diff,
                    mswd: w.mswd,
                    n: w.n,
                    status: status_for(primary_upb, diff, qa.warn_pct, qa.alarm_pct),
                });
            }
        }
        // Concordia age against the accepted 206Pb/238U age.
        if let (Some(c), Some(v)) = (&stats_row.concordia, rm.get(crate::refmat::K_R68)) {
            let accepted = d.age68(v.value);
            let diff = 100.0 * (c.age / accepted - 1.0);
            out.rows.push(QaRow {
                group: g.name.clone(),
                material: rm.name.clone(),
                quantity: "Concordia age (Ma)".to_string(),
                measured: c.age,
                measured_2s: c.unc2s,
                accepted,
                accepted_2s: d.sigma_age68(v.value, v.unc2s),
                diff_pct: diff,
                mswd: c.mswd,
                n: c.n,
                status: status_for(primary_upb, diff, qa.warn_pct, qa.alarm_pct),
            });
        }
        // Elements.
        for (channel, c) in session.inputs.channels.iter().enumerate() {
            let Some(iso) = &c.isotope else { continue };
            // The internal standard is an input here, so comparing it says nothing.
            if red.te.as_ref().is_some_and(|t| t.is_channel == Some(channel)) {
                continue;
            }
            let Some(accepted) = rm.get(&iso.element) else { continue };
            let values: Vec<(f64, f64)> = rows
                .iter()
                .filter_map(|(_, r)| r.te.get(channel)?.as_ref().map(|v| (v.ppm, v.prop2s)))
                .collect();
            let Some(w) = stats::weighted_mean(&values) else { continue };
            let cal_group = red
                .te
                .as_ref()
                .and_then(|t| t.calibrations[channel].as_ref())
                .map(|c| c.group);
            let primary = cal_group == Some(g.id);
            let diff = 100.0 * (w.mean / accepted.value - 1.0);
            out.rows.push(QaRow {
                group: g.name.clone(),
                material: rm.name.clone(),
                quantity: format!("{} (ppm)", c.name),
                measured: w.mean,
                measured_2s: w.unc2s_mswd,
                accepted: accepted.value,
                accepted_2s: accepted.unc2s,
                diff_pct: diff,
                mswd: w.mswd,
                n: w.n,
                status: status_for(primary, diff, qa.warn_pct.max(5.0), qa.alarm_pct.max(10.0)),
            });
        }
        out.groups.push(stats_row);
    }
    // Independent checks first: alarms, warnings, passes. Primaries agree by construction.
    let rank = |s: QaStatus| match s {
        QaStatus::Alarm => 0,
        QaStatus::Warn => 1,
        QaStatus::Pass => 2,
        QaStatus::Primary => 3,
    };
    out.rows.sort_by_key(|r| rank(r.status));
    out
}
