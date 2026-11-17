// SPDX-License-Identifier: GPL-3.0-or-later
//! Trace elements: external calibration by sample-standard bracketing with an
//! internal standard per selection, or semi-quantitative without one. Values
//! below the limit of detection stay as measured and are flagged, never zeroed.

use super::{Reduction, SelResult};
use crate::interp::{self, Curve};
use crate::model::*;
use crate::stats::{self, excess_for_unit_mswd};

/// A concentration with all its uncertainty types.
#[derive(Clone, Debug, Default)]
pub struct TeValue {
    pub ppm: f64,
    /// Two standard errors over the sweeps.
    pub int2se: f64,
    /// With the channel's excess uncertainty added.
    pub prop2s: f64,
    /// With the reference value's uncertainty added as well.
    pub rminc2s: f64,
    pub n: usize,
    pub n_outliers: usize,
    pub lod_longerich: f64,
    pub lod_howell: f64,
    pub lod_pettke: f64,
    /// Below the limit chosen in the settings.
    pub below_lod: bool,
    /// Calculated without an internal standard.
    pub semi_quant: bool,
}

impl TeValue {
    pub fn lod(&self, method: LodMethod) -> f64 {
        match method {
            LodMethod::Longerich => self.lod_longerich,
            LodMethod::Howell => self.lod_howell,
            LodMethod::Pettke => self.lod_pettke,
        }
    }
}

/// Calibration of one channel through time.
#[derive(Clone, Debug)]
pub struct ChannelCalibration {
    pub channel: usize,
    pub group: u64,
    /// Sensitivity through time: (ratio to internal standard) / (content ratio),
    /// or CPS per ppm when semi-quantitative.
    pub curve: Curve,
    /// Relative excess uncertainty, 1 sigma.
    pub excess: f64,
    /// Relative 2 sigma of the accepted value.
    pub rm_rel2s: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TeModel {
    pub is_channel: Option<usize>,
    pub calibrations: Vec<Option<ChannelCalibration>>,
    pub dwell_s: f64,
    pub dwell_estimated: bool,
}

fn element_of(session: &Session, channel: usize) -> Option<String> {
    session.inputs.channels[channel].isotope.as_ref().map(|i| i.element.clone())
}

/// Ratio to the internal standard (or plain CPS) per sweep of a selection.
fn sweep_signal(session: &Session, red: &Reduction, s: &Selection, channel: usize, is: Option<usize>) -> Vec<f64> {
    signal_over(session, red, session.inputs.range(s.start, s.end), channel, is)
}

fn signal_over(session: &Session, red: &Reduction, range: std::ops::Range<usize>, channel: usize, is: Option<usize>) -> Vec<f64> {
    let v = super::subtracted(session, red, channel, range.clone());
    match is {
        Some(i) if i != channel => {
            let d = super::subtracted(session, red, i, range);
            v.iter()
                .zip(&d)
                .map(|(a, b)| if *b > 0.0 { a / b } else { f64::NAN })
                .collect()
        }
        Some(_) => vec![1.0; range.len()],
        None => v,
    }
}

/// Concentration per sweep of a selection, when it can be calculated.
pub fn sweep_ppm(session: &Session, red: &Reduction, s: &Selection, channel: usize) -> Option<Vec<f64>> {
    let model = red.te.as_ref()?;
    let cal = model.calibrations.get(channel)?.as_ref()?;
    if s.excluded || session.group_of(s)?.kind == GroupKind::Baseline {
        return None;
    }
    let time = &session.inputs.time;
    let range = session.inputs.range(s.start, s.end);
    let times = &time[range];
    let signal = sweep_signal(session, red, s, channel, model.is_channel);
    let factor = match model.is_channel {
        Some(is) => session.is_value(s, &element_of(session, is)?)?,
        None => 1.0,
    };
    Some(
        signal
            .iter()
            .zip(times)
            .map(|(v, t)| v / cal.curve.eval(*t) * factor)
            .collect(),
    )
}

/// Concentration per sweep over any range of a selection's firing, for display.
pub fn ppm_over(session: &Session, red: &Reduction, s: &Selection, channel: usize, range: std::ops::Range<usize>) -> Option<Vec<f64>> {
    let model = red.te.as_ref()?;
    let cal = model.calibrations.get(channel)?.as_ref()?;
    if session.group_of(s)?.kind == GroupKind::Baseline {
        return None;
    }
    let signal = signal_over(session, red, range.clone(), channel, model.is_channel);
    let factor = match model.is_channel {
        Some(is) => session.is_value(s, &element_of(session, is)?)?,
        None => 1.0,
    };
    let times = &session.inputs.time[range];
    Some(signal.iter().zip(times).map(|(v, t)| v / cal.curve.eval(*t) * factor).collect())
}

pub fn run(session: &Session, out: &mut Reduction) {
    let settings = &session.settings.te;
    let inputs = &session.inputs;
    let n_channels = inputs.channels.len();
    let is_channel = if settings.use_internal_standard {
        settings.is_channel.as_ref().and_then(|n| inputs.channel_index(n))
    } else {
        None
    };
    if settings.use_internal_standard && is_channel.is_none() {
        out.warnings.push("Trace elements: choose an internal-standard channel, or turn it off for semi-quantitative results.".to_string());
        return;
    }
    let is_element = is_channel.and_then(|i| element_of(session, i));
    let Some(primary) = settings.primary_group else {
        out.warnings.push("Trace elements: no calibration reference group is set.".to_string());
        return;
    };
    let sweep_s = inputs.sweep_seconds();
    let dwell_estimated = settings.dwell_s.is_none();
    let dwell_s = settings
        .dwell_s
        .unwrap_or(if n_channels > 0 { sweep_s / n_channels as f64 } else { 0.0 });
    let mut model = TeModel {
        is_channel,
        calibrations: vec![None; n_channels],
        dwell_s,
        dwell_estimated,
    };
    let k = session.settings.outlier_k;
    let mut missing: Vec<String> = Vec::new();
    let mut left_out: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for channel in 0..n_channels {
        let Some(element) = element_of(session, channel) else { continue };
        if inputs.channels[channel].is_empty_signal() {
            continue;
        }
        let group = settings
            .channel_group
            .get(&inputs.channels[channel].name)
            .copied()
            .unwrap_or(primary);
        let Some(rm) = session.rm_of(group) else { continue };
        let Some(value) = rm.get(&element) else {
            // Mercury is carrier-gas background, not something to calibrate.
            if element != "Hg" && !missing.contains(&element) {
                missing.push(element);
            }
            continue;
        };
        let content = match &is_element {
            Some(e) => {
                let Some(iv) = rm.get(e) else { continue };
                value.value / iv.value
            }
            None => value.value,
        };
        let mut points: Vec<(String, f64, f64, f64)> = Vec::new();
        for s in session.selections.iter().filter(|s| s.group == group && !s.excluded) {
            let signal = sweep_signal(session, out, s, channel, is_channel);
            let m = stats::mean_se(&signal, k);
            if m.n >= 3 {
                points.push((s.label.clone(), 0.5 * (s.start + s.end), m.mean / content, m.se2 / 2.0 / content));
            }
        }
        // A reference spot far from all the others (a missed or mislabelled
        // spot) would bend the whole curve: leave it out of this channel.
        let (kept, dropped) = robust_calibration_points(points);
        for label in dropped {
            let entry = left_out.entry(label).or_insert_with(Vec::new);
            entry.push(inputs.channels[channel].name.clone());
        }
        let xs: Vec<f64> = kept.iter().map(|p| p.1).collect();
        let ys: Vec<f64> = kept.iter().map(|p| p.2).collect();
        let sg: Vec<f64> = kept.iter().map(|p| p.3).collect();
        let interp_kind = session.group(group).map_or(crate::interp::InterpKind::SmoothAuto, |g| g.interp);
        let Some(curve) = interp::fit(interp_kind, &xs, &ys, &sg) else { continue };
        let resid = interp::jackknife_residuals(interp_kind, &curve.xs, &curve.ys, &curve.sigmas);
        let (r, s): (Vec<f64>, Vec<f64>) = resid.into_iter().unzip();
        let rel = |v: f64, u: f64| if v != 0.0 { u / v } else { 0.0 };
        let rm_rel2s = {
            let a = rel(value.value, value.unc2s);
            let b = is_element
                .as_ref()
                .and_then(|e| rm.get(e))
                .map_or(0.0, |v| rel(v.value, v.unc2s));
            (a * a + b * b).sqrt()
        };
        model.calibrations[channel] = Some(ChannelCalibration {
            channel,
            group,
            excess: excess_for_unit_mswd(&r, &s),
            rm_rel2s,
            curve,
        });
    }
    for (label, channels) in &left_out {
        out.warnings.push(format!(
            "Trace elements: calibration spot {label} is far from the other spots of its reference material in {} and was left out of {} calibration. Check that spot (missed, or a different material?).",
            if channels.len() > 6 { format!("{} channels", channels.len()) } else { channels.join(", ") },
            if channels.len() == 1 { "that" } else { "those" }
        ));
        if let Some(i) = session.selections.iter().position(|s| &s.label == label) {
            out.selections[i].flags.push(format!("Left out of the {} calibration: far from the other reference spots", if channels.len() > 6 { format!("{} channels'", channels.len()) } else { channels.join(", ") }));
        }
    }
    if !missing.is_empty() {
        out.warnings.push(format!(
            "Trace elements: the calibration reference has no accepted value for {}.",
            missing.join(", ")
        ));
    }
    out.te = Some(model);
    // Per selection and channel.
    let results: Vec<(usize, Vec<Option<TeValue>>)> = session
        .selections
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.excluded && session.group_of(s).is_some_and(|g| g.kind != GroupKind::Baseline))
        .map(|(index, s)| (index, select_values(session, out, s, &out.selections[index])))
        .collect();
    let mut no_is = 0;
    for (index, values) in results {
        if values.iter().all(Option::is_none)
            && is_element
                .as_ref()
                .is_some_and(|e| session.is_value(&session.selections[index], e).is_none())
        {
            no_is += 1;
        }
        out.selections[index].te = values;
    }
    if no_is > 0 {
        out.warnings.push(format!(
            "Trace elements: {no_is} selections have no internal-standard value, so they have no concentrations yet."
        ));
    }
}

/// Splits calibration points (label, time, sensitivity, sigma) into those
/// kept and the labels of those left out: more than 30 % and more than 6
/// robust standard deviations from the median of the reference spots.
/// Needs at least 4 spots; with fewer nothing is left out.
pub fn robust_calibration_points(points: Vec<(String, f64, f64, f64)>) -> (Vec<(String, f64, f64, f64)>, Vec<String>) {
    if points.len() < 4 {
        return (points, Vec::new());
    }
    let ys: Vec<f64> = points.iter().map(|p| p.2).collect();
    let med = stats::median(&ys);
    let dev: Vec<f64> = ys.iter().map(|y| (y - med).abs()).collect();
    let mad = 1.4826 * stats::median(&dev);
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for p in points {
        let d = (p.2 - med).abs();
        let far = med != 0.0 && d > 0.30 * med.abs() && d > 6.0 * mad.max(1e-12);
        if far {
            dropped.push(p.0);
        } else {
            kept.push(p);
        }
    }
    (kept, dropped)
}

/// The closest baseline selection to a time, with mean, spread and sweep count of a channel.
fn nearest_baseline(red: &Reduction, channel: usize, t: f64) -> Option<(f64, f64, usize)> {
    let points = red.baseline_points.get(channel)?;
    points
        .iter()
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
        .map(|(_, m)| (m.mean, m.sd, m.n))
}

fn select_values(session: &Session, red: &Reduction, s: &Selection, result: &SelResult) -> Vec<Option<TeValue>> {
    let Some(model) = red.te.as_ref() else {
        return vec![None; session.inputs.channels.len()];
    };
    let k = session.settings.outlier_k;
    let method = session.settings.te.lod;
    let mid = 0.5 * (s.start + s.end);
    let is_info = model.is_channel.and_then(|i| {
        let element = element_of(session, i)?;
        let value = session.is_value(s, &element)?;
        Some((i, value))
    });
    (0..session.inputs.channels.len())
        .map(|channel| {
            let cal = model.calibrations[channel].as_ref()?;
            if model.is_channel.is_some() && is_info.is_none() {
                return None;
            }
            let values = sweep_ppm(session, red, s, channel)?;
            // the outlier rule, then the sweeps left out for this element (inclusions)
            let mut keep = stats::outlier_mask(&values, k);
            let start = session.inputs.range(s.start, s.end).start;
            for cut in result.cuts.iter().filter(|c| c.channel == channel) {
                for i in &cut.sweeps {
                    if let Some(j) = i.checked_sub(start).filter(|j| *j < keep.len()) {
                        keep[j] = false;
                    }
                }
            }
            let m = stats::mean_se_masked(&values, &keep);
            if m.n == 0 {
                return None;
            }
            let excess = 2.0 * cal.excess * m.mean.abs();
            let prop = (m.se2.powi(2) + excess * excess).sqrt();
            let rm = cal.rm_rel2s * m.mean.abs();
            // Sensitivity in CPS per ppm at this selection.
            let sensitivity = match is_info {
                Some((i, value)) => cal.curve.eval(mid) * result.cps[i].mean / value,
                None => cal.curve.eval(mid),
            };
            let n_a = result.cps[channel].n.max(1) as f64;
            let (lod_longerich, lod_howell, lod_pettke) = match nearest_baseline(red, channel, mid) {
                Some((mean_b, sd_b, n_b)) if sensitivity > 0.0 && n_b > 0 => {
                    let n_b = n_b as f64;
                    let longerich = |sd: f64| 3.0 * sd / sensitivity * (1.0 / n_b + 1.0 / n_a).sqrt();
                    let dwell = model.dwell_s;
                    let pettke = (3.29 * (mean_b.max(0.0) * dwell * n_a * (1.0 + n_a / n_b)).sqrt() + 2.71)
                        / (n_a * dwell * sensitivity);
                    (
                        longerich(sd_b),
                        longerich(if sd_b > 0.0 { sd_b } else { 7.0 }),
                        pettke,
                    )
                }
                _ => (f64::NAN, f64::NAN, f64::NAN),
            };
            let chosen = match method {
                LodMethod::Longerich => lod_longerich,
                LodMethod::Howell => lod_howell,
                LodMethod::Pettke => lod_pettke,
            };
            Some(TeValue {
                ppm: m.mean,
                int2se: m.se2,
                prop2s: prop,
                rminc2s: (prop * prop + rm * rm).sqrt(),
                n: m.n,
                n_outliers: m.n_outliers,
                lod_longerich,
                lod_howell,
                lod_pettke,
                below_lod: m.mean < chosen,
                semi_quant: model.is_channel.is_none(),
            })
        })
        .collect()
}
