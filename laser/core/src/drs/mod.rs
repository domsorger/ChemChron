// SPDX-License-Identifier: GPL-3.0-or-later
//! Reduction: input sweeps are never changed. Baselines are fitted through
//! time, each scheme calculates sweep by sweep, and statistics are taken over
//! each selection afterwards.

pub mod qa;
pub mod te;
pub mod upb;

use crate::interp::{self, Curve, InterpKind};
use crate::model::*;
use crate::stats::{self, MeanSe};
use std::ops::Range;

/// The full result of reducing a session.
#[derive(Clone, Debug, Default)]
pub struct Reduction {
    /// Baseline curve per input channel.
    pub baseline: Vec<Option<Curve>>,
    /// Quiet-time statistics for each baseline selection, per channel: (mid time, stats).
    pub baseline_points: Vec<Vec<(f64, MeanSe)>>,
    /// One per session selection, same order.
    pub selections: Vec<SelResult>,
    pub te: Option<te::TeModel>,
    pub upb: Option<upb::UpbModel>,
    pub qa: qa::QaReport,
    pub warnings: Vec<String>,
    pub elapsed_ms: f64,
}

/// One selection's results.
#[derive(Clone, Debug, Default)]
pub struct SelResult {
    pub id: u64,
    pub range: Range<usize>,
    /// Baseline-subtracted CPS per channel.
    pub cps: Vec<MeanSe>,
    /// Concentration per channel.
    pub te: Vec<Option<te::TeValue>>,
    pub upb: Option<upb::UpbValue>,
    /// Automatic checks that this analysis did not pass.
    pub flags: Vec<String>,
    /// Sweeps left out for single elements (inclusions).
    pub cuts: Vec<Cut>,
}

/// Sweeps of one selection left out for one channel only.
#[derive(Clone, Debug, Default)]
pub struct Cut {
    pub channel: usize,
    /// Found by the automatic spike removal (else drawn by hand).
    pub auto: bool,
    /// Sweep indices (into the session's time axis).
    pub sweeps: Vec<usize>,
}

/// Robust spike test for one channel of one selection: sweeps whose value
/// relative to the internal standard (or alone, without one) lies more than
/// `k` robust SD above the median. Nothing is cut when more than 40 % of the
/// sweeps would go (then it is zoning or a mixed spot, not a small inclusion)
/// or when fewer than 8 sweeps are there. Only runs of two or more sweeps
/// count; neighbouring sweeps of a run go too.
pub fn spike_sweeps(values: &[f64], is: Option<&[f64]>, k: f64) -> Vec<usize> {
    let n = values.len();
    if n < 8 || k <= 0.0 {
        return Vec::new();
    }
    let r: Vec<f64> = (0..n)
        .map(|i| match is {
            Some(d) if d[i].is_finite() && d[i] > 0.0 => values[i] / d[i],
            Some(_) => f64::NAN,
            None => values[i],
        })
        .collect();
    let finite: Vec<f64> = r.iter().copied().filter(|x| x.is_finite()).collect();
    if finite.len() < 8 {
        return Vec::new();
    }
    let med = stats::median(&finite);
    let dev: Vec<f64> = finite.iter().map(|x| (x - med).abs()).collect();
    let mad = 1.4826 * stats::median(&dev);
    if !(mad > 0.0) {
        return Vec::new();
    }
    let limit = med + k * mad;
    let raw: Vec<bool> = (0..n).map(|i| r[i].is_finite() && r[i] > limit).collect();
    // an inclusion lasts at least two sweeps in a row; single-sweep blips are
    // noise (the general outlier rule deals with those)
    let mut hit = vec![false; n];
    for i in 0..n {
        let run = raw[i] && ((i > 0 && raw[i - 1]) || (i + 1 < n && raw[i + 1]));
        if run {
            hit[i] = true;
            if i > 0 {
                hit[i - 1] = true;
            }
            if i + 1 < n {
                hit[i + 1] = true;
            }
        }
    }
    let out: Vec<usize> = (0..n).filter(|i| hit[*i]).collect();
    if out.len() * 10 > n * 4 {
        return Vec::new();
    }
    out
}

impl Reduction {
    pub fn result_for(&self, id: u64) -> Option<&SelResult> {
        self.selections.iter().find(|s| s.id == id)
    }
}

/// Baseline-subtracted values of one channel over a sweep range.
pub fn subtracted(session: &Session, red: &Reduction, channel: usize, range: Range<usize>) -> Vec<f64> {
    let c = &session.inputs.channels[channel];
    let curve = red.baseline.get(channel).and_then(|b| b.as_ref());
    range
        .map(|i| {
            let b = curve.map_or(0.0, |cv| cv.eval(session.inputs.time[i]));
            c.values[i] - b
        })
        .collect()
}

fn fit_baselines(session: &Session, out: &mut Reduction) {
    let inputs = &session.inputs;
    let baseline_selections: Vec<&Selection> = session
        .selections
        .iter()
        .filter(|s| !s.excluded && session.group_of(s).is_some_and(|g| g.kind == GroupKind::Baseline))
        .collect();
    if baseline_selections.is_empty() {
        out.warnings.push("No baseline selections: signals are used without background subtraction.".to_string());
    }
    let kind = baseline_selections
        .first()
        .and_then(|s| session.group_of(s))
        .map_or(InterpKind::StepForward, |g| g.interp);
    let k = session.settings.outlier_k;
    for c in &inputs.channels {
        let mut points = Vec::with_capacity(baseline_selections.len());
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut sg = Vec::new();
        for s in &baseline_selections {
            let r = inputs.range(s.start, s.end);
            if r.is_empty() {
                continue;
            }
            let m = stats::mean_se(&c.values[r], k);
            let mid = 0.5 * (s.start + s.end);
            xs.push(mid);
            ys.push(m.mean);
            sg.push(m.se2 / 2.0);
            points.push((mid, m));
        }
        out.baseline.push(interp::fit(kind, &xs, &ys, &sg));
        out.baseline_points.push(points);
    }
}

/// Reduce the whole session.
pub fn reduce(session: &Session) -> Reduction {
    let start = std::time::Instant::now();
    let mut out = Reduction::default();
    out.warnings.extend(session.inputs.warnings.iter().cloned());
    fit_baselines(session, &mut out);
    let n_channels = session.inputs.channels.len();
    let k = session.settings.outlier_k;
    for s in &session.selections {
        let range = session.inputs.range(s.start, s.end);
        let mut cps = Vec::with_capacity(n_channels);
        let mut cuts: Vec<Cut> = Vec::new();
        if !s.excluded {
            let te = &session.settings.te;
            let mode = te.inclusions;
            let is_index = te.is_channel.as_ref().and_then(|n| session.inputs.channel_index(n));
            let is_values = is_index.filter(|_| te.use_internal_standard).map(|i| subtracted(session, &out, i, range.clone()));
            let is_sample = session.group_of(s).is_some_and(|g| g.kind == GroupKind::Sample);
            for c in 0..n_channels {
                let v = subtracted(session, &out, c, range.clone());
                let mut keep = stats::outlier_mask(&v, k);
                let name = &session.inputs.channels[c].name;
                // inclusions first, so what they take out is shown as such
                if mode.by_hand() {
                    let mut hand = Vec::new();
                    for cut in s.cutouts.iter().filter(|x| x.channels.iter().any(|n| n == name)) {
                        for (j, i) in range.clone().enumerate() {
                            let t = session.inputs.time[i];
                            if t >= cut.start && t <= cut.end && !hand.contains(&i) {
                                keep[j] = false;
                                hand.push(i);
                            }
                        }
                    }
                    if !hand.is_empty() {
                        cuts.push(Cut { channel: c, auto: false, sweeps: hand });
                    }
                }
                // automatic removal only in unknowns: reference materials and glasses are left as they are
                if te.enabled && mode.automatic() && is_sample && Some(c) != is_index && !te.inclusion_skip.iter().any(|x| x == name) {
                    let taken: Vec<usize> = cuts.iter().filter(|x| x.channel == c).flat_map(|x| x.sweeps.iter().copied()).collect();
                    let auto: Vec<usize> = spike_sweeps(&v, is_values.as_deref(), te.inclusion_k).into_iter().filter(|j| !taken.contains(&(range.start + j))).collect();
                    if !auto.is_empty() {
                        for j in &auto {
                            keep[*j] = false;
                        }
                        cuts.push(Cut { channel: c, auto: true, sweeps: auto.iter().map(|j| range.start + j).collect() });
                    }
                }
                cps.push(stats::mean_se_masked(&v, &keep));
            }
        } else {
            cps.resize(n_channels, MeanSe::default());
        }
        out.selections.push(SelResult {
            id: s.id,
            range,
            cps,
            te: vec![None; n_channels],
            upb: None,
            flags: Vec::new(),
            cuts,
        });
    }
    if session.settings.te.enabled {
        te::run(session, &mut out);
    }
    if session.settings.upb.enabled {
        upb::run(session, &mut out);
    }
    qa::flag_selections(session, &mut out);
    out.qa = qa::report(session, &out);
    out.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    out
}

/// A time series that can be shown or exported: inputs, and each intermediate
/// and output step of the reduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeriesKey {
    Raw(usize),
    TotalBeam,
    BaselineCurve(usize),
    Subtracted(usize),
    Ppm(usize),
    BeamSeconds,
    RatioRaw(upb::RatioKind),
    RatioDownhole(upb::RatioKind),
    RatioFinal(upb::RatioKind),
    Age(upb::RatioKind),
}

impl SeriesKey {
    pub fn label(&self, session: &Session) -> String {
        let name = |c: usize| session.inputs.channels.get(c).map_or("?", |c| c.name.as_str()).to_string();
        match self {
            SeriesKey::Raw(c) => format!("{} (CPS)", name(*c)),
            SeriesKey::TotalBeam => "Total beam (CPS)".to_string(),
            SeriesKey::BaselineCurve(c) => format!("{} baseline", name(*c)),
            SeriesKey::Subtracted(c) => format!("{} minus baseline", name(*c)),
            SeriesKey::Ppm(c) => format!("{} (ppm)", name(*c)),
            SeriesKey::BeamSeconds => "Beam seconds".to_string(),
            SeriesKey::RatioRaw(r) => format!("{} raw", r.key()),
            SeriesKey::RatioDownhole(r) => format!("{} down-hole corrected", r.key()),
            SeriesKey::RatioFinal(r) => format!("{} final", r.key()),
            SeriesKey::Age(r) => format!("{} age (Ma)", r.key()),
        }
    }

    /// Shown on the main signal plot, on a counts scale. Everything else gets its own strip.
    pub fn is_counts(&self) -> bool {
        matches!(
            self,
            SeriesKey::Raw(_) | SeriesKey::TotalBeam | SeriesKey::BaselineCurve(_) | SeriesKey::Subtracted(_)
        )
    }
}

/// Calculate a whole series. Ratios, ages, beam seconds and contents cover each
/// analysis's whole laser firing, not only its selection, and are NaN elsewhere.
pub fn series(session: &Session, red: &Reduction, key: SeriesKey) -> Vec<f64> {
    let inputs = &session.inputs;
    let n = inputs.time.len();
    match key {
        SeriesKey::Raw(c) => inputs.channels[c].values.clone(),
        SeriesKey::TotalBeam => crate::autosel::total_beam(inputs),
        SeriesKey::BaselineCurve(c) => match red.baseline.get(c).and_then(|b| b.as_ref()) {
            Some(curve) => inputs.time.iter().map(|t| curve.eval(*t)).collect(),
            None => vec![0.0; n],
        },
        SeriesKey::Subtracted(c) => subtracted(session, red, c, 0..n),
        SeriesKey::Ppm(c) => {
            let mut out = vec![f64::NAN; n];
            let index = session
                .settings
                .upb
                .index_channel
                .as_ref()
                .and_then(|name| inputs.channel_index(name));
            for s in &session.selections {
                let Some(group) = session.group_of(s) else { continue };
                if group.kind == GroupKind::Baseline {
                    continue;
                }
                let range = index
                    .and_then(|i| upb::ablation_range(session, red, s, i))
                    .unwrap_or_else(|| inputs.range(s.start, s.end));
                if let Some(rows) = te::ppm_over(session, red, s, c, range.clone()) {
                    out[range].copy_from_slice(&rows);
                }
            }
            out
        }
        SeriesKey::BeamSeconds
        | SeriesKey::RatioRaw(_)
        | SeriesKey::RatioDownhole(_)
        | SeriesKey::RatioFinal(_)
        | SeriesKey::Age(_) => {
            let mut out = vec![f64::NAN; n];
            let d = &session.settings.upb.decay;
            for s in &session.selections {
                let Some((range, sw)) = upb::ablation_sweeps(session, red, s) else { continue };
                let values: Vec<f64> = match key {
                    SeriesKey::BeamSeconds => sw.tau,
                    SeriesKey::RatioRaw(k) => sw.raw[k as usize].clone(),
                    SeriesKey::RatioDownhole(k) => sw.downhole[k as usize].clone(),
                    SeriesKey::RatioFinal(k) => sw.fin[k as usize].clone(),
                    SeriesKey::Age(k) => sw.fin[k as usize]
                        .iter()
                        .map(|r| match k {
                            upb::RatioKind::R68 => d.age68(*r),
                            upb::RatioKind::R75 => d.age75(*r),
                            upb::RatioKind::R76 => d.age76(*r),
                            upb::RatioKind::R82 => d.age82(*r),
                        })
                        .collect(),
                    _ => continue,
                };
                out[range].copy_from_slice(&values);
            }
            out
        }
    }
}
