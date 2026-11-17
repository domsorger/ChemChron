// SPDX-License-Identifier: GPL-3.0-or-later
//! U-Th-Pb: raw ratios per sweep, down-hole fractionation fitted on the primary
//! reference material, normalisation through time, excess uncertainty,
//! propagated ages, and optional common-Pb and 230Th corrections.

use super::Reduction;
use crate::autosel::box5;
use crate::geochron::{self, stacey_kramers, DecayConstants};
use crate::interp::{self, Curve, InterpKind};
use crate::model::*;
use crate::refmat::{K_R68, K_R75, K_R76, K_R82};
use crate::stats::{self, excess_for_unit_mswd, mean_se_masked, outlier_mask};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RatioKind {
    R68 = 0,
    R75 = 1,
    R76 = 2,
    R82 = 3,
}

impl RatioKind {
    pub const ALL: [RatioKind; 4] = [RatioKind::R68, RatioKind::R75, RatioKind::R76, RatioKind::R82];
    pub fn key(&self) -> &'static str {
        match self {
            RatioKind::R68 => K_R68,
            RatioKind::R75 => K_R75,
            RatioKind::R76 => K_R76,
            RatioKind::R82 => K_R82,
        }
    }
    /// Short name used in settings: r68, r75, r76, r82.
    pub fn short_key(&self) -> &'static str {
        match self {
            RatioKind::R68 => "r68",
            RatioKind::R75 => "r75",
            RatioKind::R76 => "r76",
            RatioKind::R82 => "r82",
        }
    }
}

/// Down-hole fit: ratio relative to its analysis mean against beam seconds.
#[derive(Clone, Debug)]
pub struct DhfFit {
    pub model: DhfModel,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub m: f64,
    /// Beam-second bins: (centre, median of normalised ratio, sweeps in bin).
    pub bins: Vec<(f64, f64, usize)>,
    pub spline: Option<Curve>,
    pub tau_ref: f64,
    pub rss: f64,
    /// The model with the lowest Bayesian information criterion, as a suggestion.
    pub suggested: DhfModel,
}

impl DhfFit {
    /// Size of the fractionation at beam time tau, relative to the end of the
    /// fit. The corrected ratio is the raw ratio divided by this. Using the end
    /// of the fit keeps it stable when an exponential has no clear asymptote
    /// inside the ablation; normalisation to the reference removes the constant.
    pub fn factor(&self, tau: f64) -> f64 {
        let reference = self.fitted(self.tau_ref);
        if reference > 0.0 && reference.is_finite() {
            self.fitted(tau) / reference
        } else {
            1.0
        }
    }
    /// Fitted value of the ratio, relative to its analysis mean.
    pub fn fitted(&self, tau: f64) -> f64 {
        let t = tau.max(0.0);
        match self.model {
            DhfModel::None => 1.0,
            DhfModel::Linear => self.a + self.m * t,
            DhfModel::Exponential => self.a + self.b * (-self.c * t).exp(),
            DhfModel::ExponentialLinear => self.a + self.b * (-self.c * t).exp() + self.m * t,
            DhfModel::Spline => self.spline.as_ref().map_or(1.0, |s| s.eval(t)),
        }
    }
}

/// One ratio in the model.
#[derive(Clone, Debug)]
pub struct RatioModel {
    pub kind: RatioKind,
    pub dhf: Option<DhfFit>,
    /// Primary reference through time after down-hole correction.
    pub curve: Curve,
    pub accepted: f64,
    pub accepted_rel2s: f64,
    /// Relative excess uncertainty, 1 sigma: from the primary and the
    /// secondary reference materials of the session together.
    pub excess: f64,
    /// The excess the primary alone would give.
    pub excess_primary: f64,
    /// MSWD of the primary reference after excess is added.
    pub mswd_after: f64,
    /// Secondary analyses that entered the excess.
    pub n_secondary: usize,
    /// Secondary materials left out of the excess for this ratio, with why.
    pub left_out: Vec<(String, String)>,
    /// Every reference material of the session for this ratio (primary first).
    pub materials: Vec<MaterialStat>,
    /// The excess is the manual value of the lab settings.
    pub manual: bool,
}

/// One reference material's scatter in one ratio, in this session.
#[derive(Clone, Debug)]
pub struct MaterialStat {
    pub name: String,
    pub primary: bool,
    pub n: usize,
    /// MSWD with the analytical uncertainties only (the primary about its
    /// normalisation curve, a secondary about its own mean).
    pub mswd: f64,
    /// The excess (2σ %) this material alone would need.
    pub excess_2s_pct: f64,
    /// Why it is not used for the excess (None: used).
    pub left_out: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UpbModel {
    pub primary_group: u64,
    pub primary_n: usize,
    pub ratios: [Option<RatioModel>; 4],
    pub i206: usize,
    pub i207: usize,
    pub i238: usize,
    pub i208: Option<usize>,
    pub i232: Option<usize>,
    pub i204: Option<usize>,
    pub i202: Option<usize>,
    pub index_channel: usize,
}

impl UpbModel {
    pub fn ratio(&self, k: RatioKind) -> Option<&RatioModel> {
        self.ratios[k as usize].as_ref()
    }
}

/// A ratio in one selection, with every step kept for tracing back.
#[derive(Clone, Debug, Default)]
pub struct RatioValue {
    /// Mean of the raw sweep ratios.
    pub raw: f64,
    /// Mean after down-hole correction.
    pub downhole: f64,
    /// Normalisation factor at the selection's mid time.
    pub factor: f64,
    pub mean: f64,
    pub int2se: f64,
    pub prop2s: f64,
    pub rminc2s: f64,
    pub n: usize,
    pub n_outliers: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AgeValue {
    pub age: f64,
    pub int2s: f64,
    pub prop2s: f64,
    pub rminc2s: f64,
}

/// Result of a common-Pb correction.
#[derive(Clone, Debug, Default)]
pub struct CommonPbValue {
    pub method: &'static str,
    pub f206_pct: f64,
    pub r68: f64,
    pub r68_2s: f64,
    pub r75: f64,
    pub r75_2s: f64,
    pub r82: f64,
    pub r82_2s: f64,
    pub age68: f64,
    pub age68_2s: f64,
    pub age75: f64,
    pub age75_2s: f64,
    pub age82: f64,
    pub age82_2s: f64,
    pub note: String,
}

#[derive(Clone, Debug, Default)]
pub struct UpbValue {
    pub r68: RatioValue,
    pub r75: RatioValue,
    pub r76: RatioValue,
    pub r82: Option<RatioValue>,
    pub r38: f64,
    pub r38_prop2s: f64,
    /// Error correlation of 206Pb/238U with 207Pb/235U.
    pub rho_68_75: f64,
    /// Error correlation of 207Pb/206Pb with 238U/206Pb.
    pub rho_76_38: f64,
    pub a68: AgeValue,
    pub a75: AgeValue,
    pub a76: AgeValue,
    pub a82: Option<AgeValue>,
    pub discordance_pct: f64,
    pub th_u: f64,
    /// Beam time of the first sweep and the laser firing used.
    pub beam_start: f64,
    /// Share of common 206Pb from mass 204 (with Hg corrected), percent, always reported.
    pub f206_pct_204: f64,
    /// Two sigma of that share, from the counting noise on mass 204.
    pub f206_pct_204_2s: f64,
    pub common: Option<CommonPbValue>,
    /// 206Pb/238U age with the 230Th correction.
    pub a68_th230: Option<AgeValue>,
    /// Slope of the final ratio through the selection in sigma: a warning of zoning.
    pub trend_sigma: f64,
}

/// Every sweep of one selection at each step.
pub struct SweepTable {
    pub tau: Vec<f64>,
    pub raw: [Vec<f64>; 4],
    pub downhole: [Vec<f64>; 4],
    pub fin: [Vec<f64>; 4],
    pub keep: [Vec<bool>; 4],
    pub cps204: Vec<f64>,
}

/// Find the time the laser starts from the jump in the 5-point smoothed
/// index channel: the last rise through half the plateau before the selection starts.
pub fn detect_jump(inputs: &Inputs, index: &[f64], first: usize, s: &Selection) -> Option<f64> {
    let window = inputs.range(s.start - 15.0, s.start + 2.0);
    let selected = inputs.range(s.start, s.end);
    if window.len() < 6 || selected.is_empty() {
        return None;
    }
    let plateau = stats::median(&index[selected.start - first..selected.end - first]);
    if !plateau.is_finite() || plateau <= 0.0 {
        return None;
    }
    let smooth = box5(&index[window.start - first..window.end - first]);
    let threshold = 0.5 * plateau;
    let mut found = None;
    for i in 1..smooth.len() {
        if smooth[i - 1] < threshold && smooth[i] >= threshold {
            let f = (threshold - smooth[i - 1]) / (smooth[i] - smooth[i - 1]);
            let t0 = inputs.time[window.start + i - 1];
            let t1 = inputs.time[window.start + i];
            found = Some(t0 + f * (t1 - t0));
        }
    }
    found
}

fn beam_start(session: &Session, red: &Reduction, s: &Selection, index_channel: usize) -> f64 {
    let inputs = &session.inputs;
    let from_signal = || {
        let wide = inputs.range(s.start - 15.0, s.end);
        let values = super::subtracted(session, red, index_channel, wide.clone());
        detect_jump(inputs, &values, wide.start, s)
    };
    match session.settings.upb.beam {
        BeamMethod::LaserLog => s.laser_on.or_else(from_signal).unwrap_or(s.start),
        BeamMethod::SignalJump => from_signal().or(s.laser_on).unwrap_or(s.start),
    }
}

fn ratio(a: f64, b: f64) -> f64 {
    if b > 0.0 && a.is_finite() {
        a / b
    } else {
        f64::NAN
    }
}

struct RawSweeps {
    tau: Vec<f64>,
    times: Vec<f64>,
    raw: [Vec<f64>; 4],
    cps204: Vec<f64>,
}

/// All sweeps of a selection at each step, for display, export and tracing back.
pub fn sweeps_for(session: &Session, red: &Reduction, s: &Selection) -> Option<SweepTable> {
    let model = red.upb.as_ref()?;
    if s.excluded || session.group_of(s)?.kind == GroupKind::Baseline {
        return None;
    }
    let result = red.result_for(s.id)?;
    let start = result.upb.as_ref().map(|u| u.beam_start)?;
    let raw = raw_sweeps_all(session, red, s, model, start);
    let times = raw.times.clone();
    let mut table = apply_model(session, model, raw);
    cut_ratios(session, model, s, &times, &mut table);
    Some(table)
}

/// Automatic cut-outs for U–Pb (unknowns only): short runs of sweeps whose
/// 207Pb/206Pb or 204Pb signal jumps far above the spot's own level (common
/// Pb or an older inclusion) are left out of every ratio of that spot.
pub fn auto_cut_ratios(session: &Session, model: &UpbModel, s: &Selection, first: usize, table: &mut SweepTable) -> Vec<super::Cut> {
    let st = &session.settings;
    if !st.te.inclusions.automatic() || !session.group_of(s).is_some_and(|g| g.kind == GroupKind::Sample) {
        return Vec::new();
    }
    let k = st.te.inclusion_k;
    let mut hit: Vec<usize> = super::spike_sweeps(&table.raw[RatioKind::R76 as usize], None, k);
    let mut by_channel = vec![(model.i207, hit.clone())];
    if table.cps204.iter().any(|v| v.is_finite()) {
        let h204 = super::spike_sweeps(&table.cps204, None, k);
        if !h204.is_empty() {
            if let Some(i204) = model.i204 {
                by_channel.push((i204, h204.clone()));
            }
            hit.extend(h204);
        }
    }
    hit.sort();
    hit.dedup();
    for keep in table.keep.iter_mut() {
        for j in &hit {
            if let Some(x) = keep.get_mut(*j) {
                *x = false;
            }
        }
    }
    by_channel
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(channel, v)| super::Cut { channel, auto: true, sweeps: v.into_iter().map(|j| first + j).collect() })
        .collect()
}

/// Cut-outs drawn by hand on the U, Th or Pb signals leave their sweeps out of
/// every ratio that uses that channel (206/238 for Pb206 or U238, and so on).
pub fn cut_ratios(session: &Session, model: &UpbModel, s: &Selection, times: &[f64], table: &mut SweepTable) {
    if s.cutouts.is_empty() || !session.settings.te.inclusions.by_hand() {
        return;
    }
    let name = |i: usize| session.inputs.channels.get(i).map(|c| c.name.clone()).unwrap_or_default();
    let uses: [Vec<String>; 4] = [
        vec![name(model.i206), name(model.i238)],
        vec![name(model.i207), name(model.i238)],
        vec![name(model.i207), name(model.i206)],
        [model.i208, model.i232].iter().flatten().map(|i| name(*i)).collect(),
    ];
    for cut in &s.cutouts {
        for (k, chans) in uses.iter().enumerate() {
            if !cut.channels.iter().any(|c| chans.contains(c)) {
                continue;
            }
            for (j, t) in times.iter().enumerate() {
                if *t >= cut.start && *t <= cut.end {
                    if let Some(x) = table.keep[k].get_mut(j) {
                        *x = false;
                    }
                }
            }
        }
    }
}

fn raw_sweeps_all(session: &Session, red: &Reduction, s: &Selection, m: &UpbModel, start: f64) -> RawSweeps {
    let mut raw = raw_with(session, red, s, m, start);
    // Mass 204 is measured with mercury on it.
    if let Some(i204) = m.i204 {
        let range = session.inputs.range(s.start, s.end);
        let v = super::subtracted(session, red, i204, range.clone());
        raw.cps204 = match (session.settings.upb.hg_correction, m.i202) {
            (true, Some(i202)) => {
                let hg = super::subtracted(session, red, i202, range);
                let k = session.settings.upb.hg_204_202;
                v.iter().zip(&hg).map(|(a, b)| a - k * b).collect()
            }
            _ => v,
        };
    }
    raw
}

fn raw_with(session: &Session, red: &Reduction, s: &Selection, m: &UpbModel, start: f64) -> RawSweeps {
    raw_over(session, red, session.inputs.range(s.start, s.end), m, start)
}

/// The sweeps a selection's laser firing covers: from the moment the beam starts,
/// past the end of the selection for as long as the index channel holds above 5 %
/// of the selection's level. Lets a person see where to place the window.
pub fn ablation_range(session: &Session, red: &Reduction, s: &Selection, index_channel: usize) -> Option<std::ops::Range<usize>> {
    let inputs = &session.inputs;
    let selected = inputs.range(s.start, s.end);
    if selected.is_empty() {
        return None;
    }
    let start = beam_start(session, red, s, index_channel).min(s.start);
    let first = inputs.time.partition_point(|t| *t < start).min(selected.start);
    let level = stats::median(&super::subtracted(session, red, index_channel, selected.clone()));
    let limit = inputs.range(s.end, s.end + 60.0).end;
    let tail = box5(&super::subtracted(session, red, index_channel, selected.end..limit));
    let gap = (inputs.sweep_seconds() * 4.0).max(1e-6);
    let mut last = selected.end;
    if level > 0.0 {
        for (i, v) in tail.iter().enumerate() {
            let at = selected.end + i;
            let paused = at > 0 && inputs.time[at] - inputs.time[at - 1] > gap;
            if *v < 0.05 * level || paused {
                break;
            }
            last = at + 1;
        }
    }
    Some(first..last)
}

/// Every step of the ratios across a selection's whole firing, for display.
/// Left-out analyses are included, so a person can see why they were left out.
pub fn ablation_sweeps(session: &Session, red: &Reduction, s: &Selection) -> Option<(std::ops::Range<usize>, SweepTable)> {
    let model = red.upb.as_ref()?;
    let group = session.group_of(s)?;
    if group.kind == GroupKind::Baseline || session.rm_of(group.id).is_some_and(|m| m.matrix == "Glass") {
        return None;
    }
    let range = ablation_range(session, red, s, model.index_channel)?;
    let start = beam_start(session, red, s, model.index_channel);
    let raw = raw_over(session, red, range.clone(), model, start);
    Some((range, apply_model(session, model, raw)))
}

fn raw_over(session: &Session, red: &Reduction, range: std::ops::Range<usize>, m: &UpbModel, start: f64) -> RawSweeps {
    let inputs = &session.inputs;
    let d = &session.settings.upb.decay;
    let c206 = super::subtracted(session, red, m.i206, range.clone());
    let c207 = super::subtracted(session, red, m.i207, range.clone());
    let c238 = super::subtracted(session, red, m.i238, range.clone());
    let n = c206.len();
    let c232 = m.i232.map(|i| super::subtracted(session, red, i, range.clone()));
    let c208 = m.i208.map(|i| super::subtracted(session, red, i, range.clone()));
    let mut out = [
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        vec![f64::NAN; n],
    ];
    for i in 0..n {
        out[0].push(ratio(c206[i], c238[i]));
        out[1].push(ratio(c207[i] * d.u85, c238[i]));
        out[2].push(ratio(c207[i], c206[i]));
        if let (Some(a), Some(b)) = (&c208, &c232) {
            out[3][i] = ratio(a[i], b[i]);
        }
    }
    let times = inputs.time[range].to_vec();
    let tau = times.iter().map(|t| t - start).collect();
    RawSweeps { tau, times, raw: out, cps204: vec![f64::NAN; n] }
}

fn apply_model(session: &Session, model: &UpbModel, raw: RawSweeps) -> SweepTable {
    let k = session.settings.outlier_k;
    let n = raw.tau.len();
    let mut downhole: [Vec<f64>; 4] = Default::default();
    let mut fin: [Vec<f64>; 4] = Default::default();
    let mut keep: [Vec<bool>; 4] = Default::default();
    for kind in RatioKind::ALL {
        let i = kind as usize;
        let (dh, f) = match model.ratio(kind) {
            Some(rm) => {
                let dh: Vec<f64> = raw.raw[i]
                    .iter()
                    .zip(&raw.tau)
                    .map(|(r, t)| r / rm.dhf.as_ref().map_or(1.0, |d| d.factor(*t)))
                    .collect();
                let f: Vec<f64> = dh
                    .iter()
                    .zip(&raw.times)
                    .map(|(r, t)| r * rm.accepted / rm.curve.eval(*t))
                    .collect();
                (dh, f)
            }
            None => (vec![f64::NAN; n], vec![f64::NAN; n]),
        };
        keep[i] = outlier_mask(&f, k);
        downhole[i] = dh;
        fin[i] = f;
    }
    SweepTable {
        tau: raw.tau,
        raw: raw.raw,
        downhole,
        fin,
        keep,
        cps204: raw.cps204,
    }
}

fn fit_models(bins: &[(f64, f64, usize)], model: DhfModel, tau_ref: f64) -> DhfFit {
    let empty = DhfFit {
        model,
        a: 1.0,
        b: 0.0,
        c: 0.0,
        m: 0.0,
        bins: bins.to_vec(),
        spline: None,
        tau_ref,
        rss: 0.0,
        suggested: model,
    };
    if bins.len() < 3 {
        return DhfFit { model: DhfModel::None, ..empty };
    }
    let x: Vec<f64> = bins.iter().map(|b| b.0).collect();
    let y: Vec<f64> = bins.iter().map(|b| b.1).collect();
    let w: Vec<f64> = bins.iter().map(|b| b.2 as f64).collect();
    let rss = |f: &dyn Fn(f64) -> f64| -> f64 {
        (0..x.len()).map(|i| w[i] * (y[i] - f(x[i])).powi(2)).sum()
    };
    let mut fit = empty.clone();
    match model {
        DhfModel::None => {
            fit.rss = rss(&|_| 1.0);
        }
        DhfModel::Linear => {
            let (a, m) = solve2(&x, &y, &w, |_| 1.0, |t| t);
            fit.a = a;
            fit.m = m;
            fit.rss = rss(&|t| a + m * t);
        }
        DhfModel::Exponential | DhfModel::ExponentialLinear => {
            let with_line = model == DhfModel::ExponentialLinear;
            let mut best: Option<(f64, f64, f64, f64, f64)> = None;
            let evaluate = |c: f64| -> Option<(f64, f64, f64, f64, f64)> {
                let (a, b, m) = if with_line {
                    solve3(&x, &y, &w, c)?
                } else {
                    let (a, b) = solve2(&x, &y, &w, |_| 1.0, |t| (-c * t).exp());
                    (a, b, 0.0)
                };
                if a <= 0.0 {
                    return None;
                }
                let r = rss(&|t| a + b * (-c * t).exp() + m * t);
                Some((r, a, b, c, m))
            };
            for i in 0..50 {
                let c = 0.005 * (600.0f64).powf(i as f64 / 49.0);
                if let Some(v) = evaluate(c) {
                    if best.is_none_or(|b| v.0 < b.0) {
                        best = Some(v);
                    }
                }
            }
            if let Some(b) = best {
                let (mut lo, mut hi) = (b.3 * 0.75, b.3 * 1.35);
                let phi = 0.618_033_988_749_895;
                for _ in 0..30 {
                    let c1 = hi - phi * (hi - lo);
                    let c2 = lo + phi * (hi - lo);
                    let f1 = evaluate(c1).map_or(f64::INFINITY, |v| v.0);
                    let f2 = evaluate(c2).map_or(f64::INFINITY, |v| v.0);
                    if f1 < f2 {
                        hi = c2;
                    } else {
                        lo = c1;
                    }
                }
                let v = evaluate(0.5 * (lo + hi)).filter(|v| v.0 <= b.0).unwrap_or(b);
                fit.rss = v.0;
                fit.a = v.1;
                fit.b = v.2;
                fit.c = v.3;
                fit.m = v.4;
            }
        }
        DhfModel::Spline => {
            let sg: Vec<f64> = w.iter().map(|v| 1.0 / v.sqrt()).collect();
            fit.spline = interp::fit(InterpKind::SmoothAuto, &x, &y, &sg);
            let s = fit.spline.clone();
            fit.rss = rss(&|t| s.as_ref().map_or(1.0, |c| c.eval(t)));
        }
    }
    fit
}

fn solve2(x: &[f64], y: &[f64], w: &[f64], f0: impl Fn(f64) -> f64, f1: impl Fn(f64) -> f64) -> (f64, f64) {
    let (mut s00, mut s01, mut s11, mut b0, mut b1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let (a, b) = (f0(x[i]), f1(x[i]));
        s00 += w[i] * a * a;
        s01 += w[i] * a * b;
        s11 += w[i] * b * b;
        b0 += w[i] * a * y[i];
        b1 += w[i] * b * y[i];
    }
    let det = s00 * s11 - s01 * s01;
    if det.abs() < 1e-30 {
        return (b0 / s00.max(1e-30), 0.0);
    }
    ((s11 * b0 - s01 * b1) / det, (s00 * b1 - s01 * b0) / det)
}

fn solve3(x: &[f64], y: &[f64], w: &[f64], c: f64) -> Option<(f64, f64, f64)> {
    let mut m = [[0.0f64; 4]; 3];
    for i in 0..x.len() {
        let row = [1.0, (-c * x[i]).exp(), x[i]];
        for r in 0..3 {
            for k in 0..3 {
                m[r][k] += w[i] * row[r] * row[k];
            }
            m[r][3] += w[i] * row[r] * y[i];
        }
    }
    for col in 0..3 {
        let pivot = (col..3).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))?;
        m.swap(col, pivot);
        if m[col][col].abs() < 1e-12 {
            return None;
        }
        for r in 0..3 {
            if r != col {
                let f = m[r][col] / m[col][col];
                for k in col..4 {
                    m[r][k] -= f * m[col][k];
                }
            }
        }
    }
    Some((m[0][3] / m[0][0], m[1][3] / m[1][1], m[2][3] / m[2][2]))
}

/// Fit the requested model on sweeps of the primary reference, each analysis
/// first scaled to its own mean so drift through the session cannot pass for
/// fractionation. Binned medians resist single spikes.
fn fit_downhole(analyses: &[(Vec<f64>, Vec<f64>)], model: DhfModel, trim_s: f64, sweep_s: f64) -> Option<DhfFit> {
    let width = (sweep_s * 2.0).max(0.25);
    let mut bins: std::collections::BTreeMap<i64, Vec<f64>> = std::collections::BTreeMap::new();
    for (tau, values) in analyses {
        let centre = stats::median(values);
        if !centre.is_finite() || centre <= 0.0 {
            continue;
        }
        let keep = outlier_mask(values, 3.0);
        for i in 0..values.len() {
            if keep[i] && tau[i] >= trim_s {
                bins.entry((tau[i] / width).floor() as i64).or_default().push(values[i] / centre);
            }
        }
    }
    let total: usize = bins.values().map(Vec::len).sum();
    let table: Vec<(f64, f64, usize)> = bins
        .iter()
        .filter(|(_, v)| v.len() >= 3 && v.len() * 20 >= total / bins.len().max(1))
        .map(|(k, v)| ((*k as f64 + 0.5) * width, stats::median(v), v.len()))
        .collect();
    if table.len() < 3 {
        return None;
    }
    let tau_ref = table.last().map_or(0.0, |b| b.0);
    let mut fit = fit_models(&table, model, tau_ref);
    // Which model does this data prefer? Bayesian information criterion on the bins.
    let n = table.len() as f64;
    let mut best = (f64::INFINITY, DhfModel::None);
    for (candidate, parameters) in [
        (DhfModel::None, 0.0),
        (DhfModel::Linear, 2.0),
        (DhfModel::Exponential, 3.0),
        (DhfModel::ExponentialLinear, 4.0),
    ] {
        let f = fit_models(&table, candidate, tau_ref);
        let scale = table.iter().map(|b| b.2 as f64).sum::<f64>() / n;
        let bic = n * (f.rss / scale / n).max(1e-30).ln() + parameters * n.ln();
        if bic < best.0 {
            best = (bic, candidate);
        }
    }
    fit.suggested = best.1;
    Some(fit)
}

pub fn run(session: &Session, out: &mut Reduction) {
    let inputs = &session.inputs;
    let settings = &session.settings.upb;
    let (Some(i206), Some(i207), Some(i238)) = (
        inputs.find_isotope("Pb", 206),
        inputs.find_isotope("Pb", 207),
        inputs.find_isotope("U", 238),
    ) else {
        out.warnings.push("U-Th-Pb is off: it needs Pb206, Pb207 and U238 channels.".to_string());
        return;
    };
    let Some(primary_group) = settings.primary_group else {
        out.warnings.push("U-Th-Pb: choose a primary reference material on the Corrections page.".to_string());
        return;
    };
    let Some(rm) = session.rm_of(primary_group) else {
        out.warnings.push("U-Th-Pb: the primary group has no reference material assigned.".to_string());
        return;
    };
    let index_channel = settings
        .index_channel
        .as_ref()
        .and_then(|n| inputs.channel_index(n))
        .unwrap_or(i238);
    let mut model = UpbModel {
        primary_group,
        primary_n: 0,
        ratios: [None, None, None, None],
        i206,
        i207,
        i238,
        i208: inputs.find_isotope("Pb", 208),
        i232: inputs.find_isotope("Th", 232),
        i204: inputs.find_isotope("Pb", 204),
        i202: inputs.find_isotope("Hg", 202),
        index_channel,
    };
    // Beam seconds per selection.
    let mut starts = vec![f64::NAN; session.selections.len()];
    for (i, s) in session.selections.iter().enumerate() {
        // Calibration glass carries ordinary lead: its ratios would not be ages.
        let glass = session.rm_of(s.group).is_some_and(|m| m.matrix == "Glass");
        if !s.excluded && !glass && session.group_of(s).is_some_and(|g| g.kind != GroupKind::Baseline) {
            starts[i] = beam_start(session, out, s, index_channel);
        }
    }
    let primaries: Vec<usize> = (0..session.selections.len())
        .filter(|i| session.selections[*i].group == primary_group && !session.selections[*i].excluded)
        .collect();
    model.primary_n = primaries.len();
    if primaries.len() < 2 {
        out.warnings.push("U-Th-Pb: the primary reference needs at least 2 analyses.".to_string());
        return;
    }
    if primaries.len() < 15 {
        out.warnings.push(format!(
            "U-Th-Pb: {} primary analyses; at least 15 are recommended for a reliable excess uncertainty.",
            primaries.len()
        ));
    }
    let raws: Vec<RawSweeps> = primaries
        .iter()
        .map(|i| raw_with(session, out, &session.selections[*i], &model, starts[*i]))
        .collect();
    let group_interp = session.group(primary_group).map_or(InterpKind::SmoothAuto, |g| g.interp);
    let sweep_s = inputs.sweep_seconds();
    let k = session.settings.outlier_k;
    let d = settings.decay;
    let rm_r68 = rm.get(K_R68).cloned();
    let rm_r76 = rm.get(K_R76).cloned();
    let mut primary_residuals: [Vec<(f64, f64)>; 4] = Default::default();
    for kind in RatioKind::ALL {
        let i = kind as usize;
        let value = match (kind, rm.get(kind.key())) {
            (_, Some(v)) => Some(v.clone()),
            // Accepted 207Pb/235U follows from the other two when not listed.
            (RatioKind::R75, None) => match (&rm_r68, &rm_r76) {
                (Some(a), Some(b)) => Some(crate::refmat::RmValue {
                    value: a.value * b.value * d.u85,
                    unc2s: a.value * b.value * d.u85
                        * ((a.unc2s / a.value).powi(2) + (b.unc2s / b.value).powi(2)).sqrt(),
                }),
                _ => None,
            },
            _ => None,
        };
        let Some(value) = value else {
            if kind == RatioKind::R68 {
                out.warnings.push(format!(
                    "U-Th-Pb: {} has no accepted {}. Add it on the Reference materials page.",
                    rm.name,
                    kind.key()
                ));
                return;
            }
            continue;
        };
        if kind == RatioKind::R82 && (model.i208.is_none() || model.i232.is_none()) {
            continue;
        }
        let dhf_model = match kind {
            RatioKind::R68 => settings.dhf_r68,
            RatioKind::R75 => settings.dhf_r75,
            RatioKind::R82 => settings.dhf_r82,
            // 207Pb/206Pb is not down-hole corrected.
            RatioKind::R76 => DhfModel::None,
        };
        let analyses: Vec<(Vec<f64>, Vec<f64>)> =
            raws.iter().map(|r| (r.tau.clone(), r.raw[i].clone())).collect();
        let dhf = if kind == RatioKind::R76 {
            None
        } else {
            fit_downhole(&analyses, dhf_model, settings.dhf_trim_s, sweep_s)
        };
        let (mut xs, mut ys, mut sg) = (Vec::new(), Vec::new(), Vec::new());
        for (index, raw) in raws.iter().enumerate() {
            let corrected: Vec<f64> = raw.raw[i]
                .iter()
                .zip(&raw.tau)
                .map(|(r, t)| r / dhf.as_ref().map_or(1.0, |f| f.factor(*t)))
                .collect();
            let m = stats::mean_se(&corrected, k);
            if m.n >= 3 {
                let s = &session.selections[primaries[index]];
                xs.push(0.5 * (s.start + s.end));
                ys.push(m.mean);
                sg.push(m.se2 / 2.0);
            }
        }
        let Some(curve) = interp::fit(group_interp, &xs, &ys, &sg) else { continue };
        let residuals = interp::jackknife_residuals(group_interp, &curve.xs, &curve.ys, &curve.sigmas);
        let (r, s): (Vec<f64>, Vec<f64>) = residuals.into_iter().unzip();
        let excess = excess_for_unit_mswd(&r, &s);
        let dof = r.len().saturating_sub(1).max(1) as f64;
        let mswd_after = r
            .iter()
            .zip(&s)
            .map(|(r, s)| r * r / (s * s + excess * excess))
            .sum::<f64>()
            / dof;
        model.ratios[i] = Some(RatioModel {
            kind,
            dhf,
            curve,
            accepted: value.value,
            accepted_rel2s: if value.value != 0.0 { value.unc2s / value.value } else { 0.0 },
            excess,
            excess_primary: excess,
            mswd_after,
            n_secondary: 0,
            left_out: Vec::new(),
            manual: false,
            materials: Vec::new(),
        });
        primary_residuals[i] = r.into_iter().zip(s).collect();
    }
    if model.ratio(RatioKind::R68).is_none() || model.ratio(RatioKind::R76).is_none() {
        out.warnings.push(format!("U-Th-Pb: {} needs accepted 206Pb/238U and 207Pb/206Pb.", rm.name));
        return;
    }
    if model.ratio(RatioKind::R82).is_none() && model.i208.is_some() && model.i232.is_some() {
        out.warnings.push(format!(
            "U-Th-Pb: {} has no accepted 208Pb/232Th, so Th-Pb ages are not calculated.",
            rm.name
        ));
    }
    // The secondary reference materials of the session join the excess: each
    // scatters about its own mean (their offsets from the accepted values are
    // checked on the Quality page and belong to the long-term systematic term).
    let secondaries: Vec<usize> = (0..session.selections.len())
        .filter(|i| starts[*i].is_finite())
        .filter(|i| {
            let s = &session.selections[*i];
            s.group != primary_group && session.group_of(s).is_some_and(|g| g.kind == GroupKind::ReferenceMaterial)
        })
        .collect();
    let mut by_group: Vec<(u64, Vec<[Option<(f64, f64)>; 4]>)> = Vec::new();
    for &i in &secondaries {
        let s = &session.selections[i];
        let raw = raw_sweeps_all(session, out, s, &model, starts[i]);
        let times = raw.times.clone();
        let mut table = apply_model(session, &model, raw);
        cut_ratios(session, &model, s, &times, &mut table);
        let _ = auto_cut_ratios(session, &model, s, out.selections[i].range.start, &mut table);
        let mut v: [Option<(f64, f64)>; 4] = [None; 4];
        for k in RatioKind::ALL {
            let m = mean_se_masked(&table.fin[k as usize], &table.keep[k as usize]);
            if m.n >= 3 && m.mean.is_finite() && m.mean != 0.0 {
                v[k as usize] = Some((m.mean, m.se2 / 2.0));
            }
        }
        match by_group.iter_mut().find(|g| g.0 == s.group) {
            Some(g) => g.1.push(v),
            None => by_group.push((s.group, vec![v])),
        }
    }
    let rule = &settings.excess_rule;
    // secondaries younger than 1000 Ma (from their mean 206Pb/238U)
    let young_materials: Vec<String> = by_group
        .iter()
        .filter_map(|(g, list)| {
            let r: Vec<f64> = list.iter().filter_map(|v| v[RatioKind::R68 as usize].map(|x| x.0)).collect();
            if r.is_empty() {
                return None;
            }
            let mean = r.iter().sum::<f64>() / r.len() as f64;
            let age_ma = (1.0 + mean).ln() / d.l238 / 1e6;
            (age_ma < 1000.0).then(|| session.rm_of(*g).map(|m| m.name.clone()).or_else(|| session.group(*g).map(|x| x.name.clone())).unwrap_or_default())
        })
        .collect();
    let name_of = |g: u64| session.rm_of(g).map(|m| m.name.clone()).or_else(|| session.group(g).map(|x| x.name.clone())).unwrap_or_default();
    for k in RatioKind::ALL {
        let i = k as usize;
        let Some(rm) = model.ratios[i].as_mut() else { continue };
        // every material's own scatter, for the settings page
        let mswd_of = |g: &[(f64, f64)], about_mean: bool| -> f64 {
            if g.len() < 2 {
                return f64::NAN;
            }
            let w: Vec<f64> = g.iter().map(|(_, s)| 1.0 / (s * s)).collect();
            let m = if about_mean { g.iter().zip(&w).map(|((x, _), w)| x * w).sum::<f64>() / w.iter().sum::<f64>() } else { 0.0 };
            g.iter().zip(&w).map(|((x, _), w)| (x - m).powi(2) * w).sum::<f64>() / (g.len() - 1) as f64
        };
        let mut stats_list = vec![MaterialStat {
            name: name_of(primary_group),
            primary: true,
            n: primary_residuals[i].len(),
            mswd: mswd_of(&primary_residuals[i], false),
            excess_2s_pct: 200.0 * rm.excess_primary,
            left_out: None,
        }];
        if let Some(manual) = rule.manual_2s_pct {
            // the lab's manual value
            rm.excess = manual[i] / 200.0;
            rm.manual = true;
            rm.materials = stats_list;
            continue;
        }
        // each secondary about its own mean, relative
        let mut mats: Vec<(String, Vec<(f64, f64)>)> = by_group
            .iter()
            .map(|(g, list)| {
                let vals: Vec<(f64, f64)> = list.iter().filter_map(|v| v[i]).collect();
                let mean = vals.iter().map(|x| x.0).sum::<f64>() / vals.len().max(1) as f64;
                (name_of(*g), vals.iter().map(|(x, sg)| (x / mean, sg / mean.abs())).collect::<Vec<_>>())
            })
            .filter(|(_, g)| g.len() >= 2)
            .collect();
        for (n, g) in &mats {
            stats_list.push(MaterialStat { name: n.clone(), primary: false, n: g.len(), mswd: mswd_of(g, true), excess_2s_pct: 200.0 * stats::pooled_excess(&[], std::slice::from_ref(g)), left_out: None });
        }
        // left out by hand
        let mut left: Vec<(String, String)> = Vec::new();
        mats.retain(|(n, _)| {
            let out = rule.excludes(n, k.short_key());
            if out {
                left.push((n.clone(), "left out by hand".into()));
            }
            !out
        });
        // the ²⁰⁷Pb/²⁰⁶Pb of a young material (Pb–Pb age too imprecise) is not trusted
        if k == RatioKind::R76 {
            mats.retain(|(n, _)| {
                let young = young_materials.contains(n);
                if young {
                    left.push((n.clone(), "younger than 1000 Ma: its 207Pb/206Pb is too imprecise to be trusted".into()));
                }
                !young
            });
        }
        let groups: Vec<Vec<(f64, f64)>> = mats.into_iter().map(|x| x.1).collect();
        let n_sec: usize = groups.iter().map(|g| g.len()).sum();
        if n_sec > 0 {
            rm.excess = stats::pooled_excess(&primary_residuals[i], &groups);
        }
        rm.n_secondary = n_sec;
        for st in stats_list.iter_mut() {
            st.left_out = left.iter().find(|(n, _)| *n == st.name).map(|x| x.1.clone());
        }
        rm.left_out = left;
        rm.materials = stats_list;
    }
    // Every analysis.
    let values: Vec<(usize, UpbValue, Vec<super::Cut>)> = (0..session.selections.len())
        .filter(|i| starts[*i].is_finite())
        .filter_map(|i| {
            let s = &session.selections[i];
            let raw = raw_sweeps_all(session, out, s, &model, starts[i]);
            let times = raw.times.clone();
            let mut table = apply_model(session, &model, raw);
            cut_ratios(session, &model, s, &times, &mut table);
            let cuts = auto_cut_ratios(session, &model, s, out.selections[i].range.start, &mut table);
            let value = analysis_value(session, out, s, i, &model, &table, &times, starts[i], &d)?;
            Some((i, value, cuts))
        })
        .collect();
    for (i, v, cuts) in values {
        out.selections[i].upb = Some(v);
        out.selections[i].cuts.extend(cuts);
    }
    out.upb = Some(model);
}

fn ratio_value(model: &UpbModel, table: &SweepTable, kind: RatioKind, mid: f64) -> Option<RatioValue> {
    let rm = model.ratio(kind)?;
    let i = kind as usize;
    let m = mean_se_masked(&table.fin[i], &table.keep[i]);
    if m.n == 0 {
        return None;
    }
    let raw = mean_se_masked(&table.raw[i], &table.keep[i]);
    let dh = mean_se_masked(&table.downhole[i], &table.keep[i]);
    let excess = 2.0 * rm.excess * m.mean.abs();
    let prop = (m.se2.powi(2) + excess * excess).sqrt();
    let rm_part = rm.accepted_rel2s * m.mean.abs();
    Some(RatioValue {
        raw: raw.mean,
        downhole: dh.mean,
        factor: rm.accepted / rm.curve.eval(mid),
        mean: m.mean,
        int2se: m.se2,
        prop2s: prop,
        rminc2s: (prop * prop + rm_part * rm_part).sqrt(),
        n: m.n,
        n_outliers: m.n_outliers,
    })
}

#[allow(clippy::too_many_arguments)]
fn analysis_value(
    session: &Session,
    red: &Reduction,
    s: &Selection,
    index: usize,
    model: &UpbModel,
    table: &SweepTable,
    _times: &[f64],
    start: f64,
    d: &DecayConstants,
) -> Option<UpbValue> {
    let mid = 0.5 * (s.start + s.end);
    let r68 = ratio_value(model, table, RatioKind::R68, mid)?;
    let r76 = ratio_value(model, table, RatioKind::R76, mid)?;
    let r75 = match ratio_value(model, table, RatioKind::R75, mid) {
        Some(v) => v,
        None => return None,
    };
    let r82 = ratio_value(model, table, RatioKind::R82, mid);
    let age = |t: f64, si: f64, sp: f64, sr: f64| AgeValue { age: t, int2s: si, prop2s: sp, rminc2s: sr };
    let a68 = age(
        d.age68(r68.mean),
        d.sigma_age68(r68.mean, r68.int2se),
        d.sigma_age68(r68.mean, r68.prop2s),
        d.sigma_age68(r68.mean, r68.rminc2s),
    );
    let a75 = age(
        d.age75(r75.mean),
        d.sigma_age75(r75.mean, r75.int2se),
        d.sigma_age75(r75.mean, r75.prop2s),
        d.sigma_age75(r75.mean, r75.rminc2s),
    );
    let a76 = age(
        d.age76(r76.mean),
        d.sigma_age76(r76.mean, r76.int2se),
        d.sigma_age76(r76.mean, r76.prop2s),
        d.sigma_age76(r76.mean, r76.rminc2s),
    );
    let a82 = r82.as_ref().map(|r| {
        age(
            d.age82(r.mean),
            d.sigma_age82(r.mean, r.int2se),
            d.sigma_age82(r.mean, r.prop2s),
            d.sigma_age82(r.mean, r.rminc2s),
        )
    });
    // Error correlations over the sweeps both ratios keep.
    let both_6875: Vec<bool> = table.keep[0].iter().zip(&table.keep[1]).map(|(a, b)| *a && *b).collect();
    let rho_68_75 = stats::correlation(&table.fin[0], &table.fin[1], &both_6875);
    let r38_sweeps: Vec<f64> = table.fin[0].iter().map(|v| 1.0 / v).collect();
    let both_7638: Vec<bool> = table.keep[2].iter().zip(&table.keep[0]).map(|(a, b)| *a && *b).collect();
    let rho_76_38 = stats::correlation(&table.fin[2], &r38_sweeps, &both_7638);
    let result = &red.selections[index];
    let th_u = match model.i232 {
        Some(i) => result.cps[i].mean / result.cps[model.i238].mean,
        None => f64::NAN,
    };
    // Common 206Pb seen from mass 204, as a check for every analysis.
    let c204 = stats::mean_se(&table.cps204, session.settings.outlier_k);
    let cps206 = result.cps[model.i206].mean;
    let (f206_pct_204, f206_pct_204_2s) = {
        let (a, _, _) = composition(session, a68.age, d);
        (100.0 * (c204.mean / cps206 * a), 100.0 * (c204.se2 / cps206 * a))
    };
    let group = session.group_of(s);
    let common = common_correction(
        session, model, result, &r68, &r75, r82.as_ref(), &r76, &c204, a68.age, th_u, d,
    );
    let th230_age = (|| {
        if !session.settings.upb.th230_enabled {
            return None;
        }
        let melt = group?.th_u_melt?;
        if !th_u.is_finite() || melt <= 0.0 {
            return None;
        }
        let base = common.as_ref().map_or(r68.mean, |c| c.r68);
        let base_s = common.as_ref().map_or(r68.prop2s, |c| c.r68_2s);
        let r = geochron::th230_corrected_r68(base, th_u, melt, d);
        Some(AgeValue {
            age: d.age68(r),
            int2s: d.sigma_age68(r, r68.int2se),
            prop2s: d.sigma_age68(r, base_s),
            rminc2s: d.sigma_age68(r, r68.rminc2s.max(base_s)),
        })
    })();
    let trend_sigma = trend_in_sigma(&table.tau, &table.fin[0], &table.keep[0]);
    Some(UpbValue {
        r38: 1.0 / r68.mean,
        r38_prop2s: r68.prop2s / (r68.mean * r68.mean),
        rho_68_75,
        rho_76_38,
        discordance_pct: geochron::discordance(a68.age, a75.age, a76.age),
        th_u,
        beam_start: start,
        f206_pct_204,
        f206_pct_204_2s,
        a68,
        a75,
        a76,
        a82,
        r68,
        r75,
        r76,
        r82,
        common,
        a68_th230: th230_age,
        trend_sigma,
    })
}

fn composition(session: &Session, age: f64, d: &DecayConstants) -> (f64, f64, f64) {
    match session.settings.upb.common_composition {
        CommonPbComposition::StaceyKramers => stacey_kramers(age, d),
        CommonPbComposition::User(a, b, c) => (a, b, c),
    }
}

#[allow(clippy::too_many_arguments)]
fn common_correction(
    session: &Session,
    model: &UpbModel,
    result: &super::SelResult,
    r68: &RatioValue,
    r75: &RatioValue,
    r82: Option<&RatioValue>,
    r76: &RatioValue,
    c204: &stats::MeanSe,
    age: f64,
    th_u: f64,
    d: &DecayConstants,
) -> Option<CommonPbValue> {
    let method = session.settings.upb.common_method;
    let pair = |r: f64, s: f64, f: f64, sf: f64| -> (f64, f64) {
        let v = r * (1.0 - f);
        (v, ((s * (1.0 - f)).powi(2) + (r * sf).powi(2)).sqrt())
    };
    match method {
        CommonPbMethod::None => None,
        CommonPbMethod::Pb204 => {
            if !c204.mean.is_finite() {
                return None;
            }
            let cps = |i: Option<usize>| i.map_or(f64::NAN, |c| result.cps[c].mean);
            let (c206, c207, c208) = (cps(Some(model.i206)), cps(Some(model.i207)), cps(model.i208));
            let rel204 = (c204.se2 / c204.mean).abs();
            let mut t = age;
            let (mut f6, mut f7, mut f8) = (0.0, 0.0, 0.0);
            for _ in 0..8 {
                let (a, b, c) = composition(session, t, d);
                f6 = c204.mean * a / c206;
                f7 = c204.mean * b / c207;
                f8 = c204.mean * c / c208;
                t = d.age68(r68.mean * (1.0 - f6));
                if !t.is_finite() {
                    return None;
                }
            }
            let (v68, s68) = pair(r68.mean, r68.prop2s, f6, f6.abs() * rel204);
            let (v75, s75) = pair(r75.mean, r75.prop2s, f7, f7.abs() * rel204);
            let (v82, s82) = match r82 {
                Some(r) => pair(r.mean, r.prop2s, f8, f8.abs() * rel204),
                None => (f64::NAN, f64::NAN),
            };
            Some(CommonPbValue {
                method: "204Pb",
                f206_pct: 100.0 * f6,
                r68: v68,
                r68_2s: s68,
                r75: v75,
                r75_2s: s75,
                r82: v82,
                r82_2s: s82,
                age68: d.age68(v68),
                age68_2s: d.sigma_age68(v68, s68),
                age75: d.age75(v75),
                age75_2s: d.sigma_age75(v75, s75),
                age82: d.age82(v82),
                age82_2s: d.sigma_age82(v82, s82),
                note: String::new(),
            })
        }
        CommonPbMethod::Pb207 => {
            let (v68, t, f) = geochron::correct_207(
                r68.mean,
                r76.mean,
                |t| {
                    let (a, b, _) = composition(session, t, d);
                    b / a
                },
                d,
            )?;
            let s68 = r68.prop2s * (1.0 - f);
            let v75 = d.r76(t) * v68 * d.u85;
            let rel = (r76.prop2s / r76.mean).hypot(s68 / v68);
            Some(CommonPbValue {
                method: "207Pb",
                f206_pct: 100.0 * f,
                r68: v68,
                r68_2s: s68,
                r75: v75,
                r75_2s: v75 * rel,
                r82: f64::NAN,
                r82_2s: f64::NAN,
                age68: t,
                age68_2s: d.sigma_age68(v68, s68),
                age75: d.age75(v75),
                age75_2s: d.sigma_age75(v75, v75 * rel),
                age82: f64::NAN,
                age82_2s: f64::NAN,
                note: "Assumes concordance".to_string(),
            })
        }
        CommonPbMethod::Pb208 => {
            let r82 = r82?;
            if !th_u.is_finite() || th_u <= 0.0 {
                return None;
            }
            let (a, _, c) = composition(session, age, d);
            let (x, f) = geochron::correct_208(r68.mean, r82.mean, th_u, c / a, d)?;
            let c86 = c / a;
            let v68 = r68.mean - x;
            let s68 = r68.prop2s * (1.0 - f);
            let v82 = r82.mean - x * c86 / th_u;
            Some(CommonPbValue {
                method: "208Pb",
                f206_pct: 100.0 * f,
                r68: v68,
                r68_2s: s68,
                r75: f64::NAN,
                r75_2s: f64::NAN,
                r82: v82,
                r82_2s: r82.prop2s,
                age68: d.age68(v68),
                age68_2s: d.sigma_age68(v68, s68),
                age75: f64::NAN,
                age75_2s: f64::NAN,
                age82: d.age82(v82),
                age82_2s: d.sigma_age82(v82, r82.prop2s),
                note: "Assumes Th-U concordance and uses count-rate Th/U".to_string(),
            })
        }
    }
}

/// How many standard errors the slope of a ratio over beam time is from flat.
fn trend_in_sigma(tau: &[f64], values: &[f64], keep: &[bool]) -> f64 {
    let (mut n, mut sx, mut sy) = (0.0, 0.0, 0.0);
    for i in 0..tau.len() {
        if keep[i] {
            n += 1.0;
            sx += tau[i];
            sy += values[i];
        }
    }
    if n < 5.0 {
        return 0.0;
    }
    let (mx, my) = (sx / n, sy / n);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for i in 0..tau.len() {
        if keep[i] {
            sxx += (tau[i] - mx).powi(2);
            sxy += (tau[i] - mx) * (values[i] - my);
        }
    }
    if sxx <= 0.0 {
        return 0.0;
    }
    let b = sxy / sxx;
    let mut sse = 0.0;
    for i in 0..tau.len() {
        if keep[i] {
            sse += (values[i] - my - b * (tau[i] - mx)).powi(2);
        }
    }
    let se = (sse / (n - 2.0) / sxx).sqrt();
    if se > 0.0 {
        b / se
    } else {
        0.0
    }
}

pub use geochron::{concordia_age, ConcordiaPoint};
