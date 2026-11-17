// SPDX-License-Identifier: GPL-3.0-or-later
//! Statistics for isotopic U-Pb ages (one laser spot counted once,
//! discordant/imprecise spots left out), the U-Pb uncertainty model (random
//! excess on the ratios per session, systematic term) and the U-Pb
//! long-term log.

use crate::constants::{DecayConstants, DEFAULT_MINERAL, MINERAL_NAMES};
use crate::reference::{Refs, OUTSIDE_2S, WITHIN_2S};
use crate::row::*;
use crate::sessions::upb_session;
use crate::stats::{iteratively_reject, mineral_of, WeightedMean, MIN_N_FOR_WEIGHTED_MEAN};
use crate::trace::grain_key;
use crate::uncertainty::{between_session_systematic, excess_for_unit_mswd, MIN_DOF_FOR_POOLED_EXCESS};
use crate::upb::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const AGE_TYPES: [&str; 4] = ["t68", "t75", "t76", "t82"];
pub const RATIOS: [&str; 3] = ["r86", "r76", "r82"];
pub const T76_PREFERRED_ABOVE_MA: f64 = 1000.0;

pub fn age_type_label(t: &str) -> &'static str {
    match t {
        "t68" => "²⁰⁶Pb/²³⁸U",
        "t75" => "²⁰⁷Pb/²³⁵U",
        "t76" => "²⁰⁷Pb/²⁰⁶Pb",
        "t82" => "²⁰⁸Pb/²³²Th",
        "concordia" => "Concordia",
        _ => "?",
    }
}

pub fn ratio_label(r: &str) -> &'static str {
    match r {
        "r86" => "²³⁸U/²⁰⁶Pb",
        "r76" => "²⁰⁷Pb/²⁰⁶Pb",
        _ => "²⁰⁸Pb/²³²Th",
    }
}

fn spot_key(row: &Row) -> String {
    let mut k = grain_key(&text(row, "Label"));
    for f in ["UPb_r86", "UPb_r76", "UPb_r68", "UPb_r75", "UPb_r82"] {
        k.push('|');
        k.push_str(&n(row, f).map(|v| format!("{v:e}")).unwrap_or_default());
    }
    k
}

/// Indices of included rows with U-Pb data, each laser spot once.
pub fn unique_spots(rows: &[&Row]) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if !has_upb(r) || !included_in_upb(r) {
            continue;
        }
        if seen.insert(spot_key(r)) {
            out.push(i);
        }
    }
    out
}

/// (age, 1 sigma) on the point uncertainty when present, else the imported 2SE.
pub fn age_and_sigma(row: &Row, t: &str) -> Option<(f64, f64)> {
    let age = n(row, &format!("UPb_{t}_Ma"))?;
    let s2 = n(row, &format!("UPb_{t}_2s_point_Ma")).or_else(|| n(row, &format!("UPb_{t}_2s_Ma")))?;
    if s2 <= 0.0 {
        return None;
    }
    Some((age, s2 / 2.0))
}

pub fn preferred_age_type(rows: &[&Row]) -> &'static str {
    let ages: Vec<f64> = rows.iter().filter_map(|r| n(r, "UPb_t68_Ma")).collect();
    if !ages.is_empty() && median(&ages) > T76_PREFERRED_ABOVE_MA {
        "t76"
    } else {
        "t68"
    }
}

pub struct Split {
    /// (row index, age, sigma 1s)
    pub used: Vec<(usize, f64, f64)>,
    pub rejected: Vec<(usize, f64, f64)>,
    pub n_duplicates: usize,
}

pub fn split_spots(rows: &[&Row], t: &str, c: &Criteria) -> Split {
    let with = rows.iter().filter(|r| has_upb(r) && included_in_upb(r)).count();
    let spots = unique_spots(rows);
    let mut s = Split { used: Vec::new(), rejected: Vec::new(), n_duplicates: with - spots.len() };
    for i in spots {
        let Some((a, sg)) = age_and_sigma(rows[i], t) else { continue };
        if upb_rejection(rows[i], c).is_some() {
            s.rejected.push((i, a, sg));
        } else {
            s.used.push((i, a, sg));
        }
    }
    s
}

#[derive(Clone, Debug, Default)]
pub struct IsoMean {
    pub wm: WeightedMean,
    pub discordant: Vec<usize>,
    pub n_duplicates: usize,
    pub systematic_pct: f64,
}

pub fn isotopic_weighted_mean(rows: &[&Row], t: &str, systematic_pct: f64, reject: bool, k: f64, c: &Criteria) -> Option<IsoMean> {
    let sp = split_spots(rows, t, c);
    let mut tr = sp.used.clone();
    let mut rej = Vec::new();
    if reject {
        let (kk, r) = iteratively_reject(tr, k);
        tr = kk;
        rej = r;
    }
    if tr.len() < MIN_N_FOR_WEIGHTED_MEAN {
        return None;
    }
    let ages: Vec<f64> = tr.iter().map(|x| x.1).collect();
    let sig: Vec<f64> = tr.iter().map(|x| x.2).collect();
    let mut wm = crate::stats::weighted_mean(&ages, &sig, false, k)?;
    wm.used_idx = tr.iter().map(|x| x.0).collect();
    wm.rejected_idx = rej.iter().map(|x| x.0).collect();
    wm.rejected = rej.iter().map(|x| (x.1, x.2)).collect();
    let sys = systematic_pct / 100.0 * wm.mean;
    wm.long_term_2s = sys;
    wm.total_2s = (wm.expanded_2s.powi(2) + sys * sys).sqrt();
    wm.total_2s_plain = (wm.internal_2s.powi(2) + sys * sys).sqrt();
    Some(IsoMean { wm, discordant: sp.rejected.iter().map(|x| x.0).collect(), n_duplicates: sp.n_duplicates, systematic_pct })
}

pub fn isotopic_group_stats(rows: &[&Row], t: &str, _dp: usize, systematic_pct: f64, reject: bool, k: f64, c: &Criteria) -> Vec<(String, Value)> {
    if !rows.iter().any(|r| has_upb(r)) {
        return Vec::new();
    }
    let sp = split_spots(rows, t, c);
    let ages: Vec<f64> = sp.used.iter().map(|x| x.1).collect();
    let mut out = vec![
        ("UPb_N".to_string(), jnum(ages.len() as f64)),
        ("UPb_N_disc".into(), jnum(sp.rejected.len() as f64)),
        ("UPb_Mean_Ma".into(), blank()),
        ("UPb_2s_Ma".into(), blank()),
        ("UPb_WM_Ma".into(), blank()),
        ("UPb_WM_2s_Ma".into(), blank()),
        ("UPb_MSWD".into(), blank()),
    ];
    if !ages.is_empty() {
        out[2].1 = jnum(mean(&ages));
        out[3].1 = jnum(if ages.len() > 1 { 2.0 * pstdev(&ages) } else { 0.0 });
    }
    if let Some(w) = isotopic_weighted_mean(rows, t, systematic_pct, reject, k, c) {
        out[4].1 = jnum(w.wm.mean);
        out[5].1 = jnum(w.wm.total_2s);
        out[6].1 = jnum(w.wm.mswd);
    }
    out
}

/// The reference age of a material (one age per material, used for
/// chemical and U–Pb ages alike); `t` is kept for the callers.
pub fn accepted_upb_age(refs: &Refs, code: &str, _t: &str) -> (Option<f64>, Option<f64>) {
    let Some(r) = refs.get(code) else { return (None, None) };
    let g = |k: &str| r.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())));
    (g("age"), g("age_2s"))
}

pub const UPB_REF_FIELDS: [&str; 5] = ["UPb_Ref_Accepted_Ma", "UPb_Ref_Accepted_2s_Ma", "UPb_Ref_Diff_Ma", "UPb_Ref_Diff_pct", "UPb_Ref_Within_2s"];

pub fn upb_reference_fields(row: &Row, refs: &Refs, t: &str, systematic_pct: f64, _dp: usize) -> Vec<(&'static str, Value)> {
    let mut out: Vec<(&'static str, Value)> = UPB_REF_FIELDS.iter().map(|k| (*k, blank())).collect();
    if !truthy(row, "Is_Reference") || !has_upb(row) {
        return out;
    }
    let (acc, acc2) = accepted_upb_age(refs, &text(row, "Reference"), t);
    let (Some(acc), Some((age, s))) = (acc, age_and_sigma(row, t)) else { return out };
    let diff = age - acc;
    let sys = systematic_pct / 100.0 * age;
    let comb = ((2.0 * s).powi(2) + sys * sys + acc2.unwrap_or(0.0).powi(2)).sqrt();
    out[0].1 = jnum(acc);
    out[1].1 = jopt(acc2);
    out[2].1 = jnum(diff);
    out[3].1 = if acc != 0.0 { jnum(100.0 * diff / acc) } else { blank() };
    out[4].1 = jstr(if diff.abs() <= comb { WITHIN_2S } else { OUTSIDE_2S });
    out
}

pub fn upb_overview_reference_fields(wm: Option<f64>, refs: &Refs, code: &str, t: &str, _dp: usize) -> Vec<(String, Value)> {
    let (acc, acc2) = accepted_upb_age(refs, code, t);
    let mut out = vec![
        ("UPb_Accepted_Ma".to_string(), blank()),
        ("UPb_Accepted_2s_Ma".into(), blank()),
        ("UPb_Diff_Ma".into(), blank()),
        ("UPb_Diff_pct".into(), blank()),
    ];
    let Some(acc) = acc else { return out };
    out[0].1 = jnum(acc);
    out[1].1 = jopt(acc2);
    if let Some(w) = wm {
        let d = w - acc;
        out[2].1 = jnum(d);
        out[3].1 = if acc != 0.0 { jnum(100.0 * d / acc) } else { blank() };
    }
    out
}

/// TW pair with point uncertainties when present.
pub fn tw_pair_point(row: &Row) -> TwPair {
    let mut p = tw_pair(row);
    if let (Some(px), Some(py)) = (n(row, "UPb_r86_2s_point"), n(row, "UPb_r76_2s_point")) {
        p.sx = Some(px);
        p.sy = Some(py);
        if let Some(r) = n(row, "UPb_rho_TW_point") {
            p.rho = Some(r);
        }
    }
    p
}

/// (207/235, 2s, 206/238, 2s, rho), imported or derived from the TW pair.
pub fn wetherill_pair(row: &Row) -> Option<(f64, Option<f64>, f64, Option<f64>, Option<f64>)> {
    let (r75, s75, r68, s68) = (n(row, "UPb_r75"), n(row, "UPb_r75_2s"), n(row, "UPb_r68"), n(row, "UPb_r68_2s"));
    let has_point = n(row, "UPb_r86_2s_point").is_some();
    if let (Some(a), Some(b)) = (r75, r68) {
        if !has_point {
            return Some((a, s75, b, s68, n(row, "UPb_rho_W")));
        }
    }
    let p = tw_pair_point(row);
    let (x, y) = (p.x.filter(|v| *v > 0.0)?, p.y.filter(|v| *v > 0.0)?);
    let r68 = 1.0 / x;
    let r75 = y * U238_U235 / x;
    if let (Some(sx), Some(sy)) = (p.sx, p.sy) {
        let (a, b, c) = (sy / y, sx / x, p.rho.unwrap_or(0.0));
        let rel = (a * a + b * b - 2.0 * c * a * b).max(0.0).sqrt();
        let rho = if b > 0.0 && rel > 0.0 { Some((b * b - c * a * b) / (b * rel)) } else { None };
        return Some((r75, Some(r75 * rel), r68, Some(r68 * b), rho));
    }
    Some((r75, None, r68, None, None))
}

/// (row index, x, sx2, y, sy2, rho) of the unique spots with a usable pair.
pub fn concordia_data(rows: &[&Row], wetherill: bool) -> Vec<(usize, f64, f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for i in unique_spots(rows) {
        let r = rows[i];
        let v = if wetherill {
            wetherill_pair(r).and_then(|(x, sx, y, sy, rho)| Some((x, sx?, y, sy?, rho.unwrap_or(0.0))))
        } else {
            let p = tw_pair_point(r);
            (|| Some((p.x?, p.sx?, p.y?, p.sy?, p.rho.unwrap_or(0.0))))()
        };
        if let Some((x, sx, y, sy, rho)) = v {
            if x > 0.0 && y > 0.0 {
                out.push((i, x, sx, y, sy, rho));
            }
        }
    }
    out
}

// ---- uncertainty model ---------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpbMineralUncertainty {
    /// "project": from the reference materials of each session (primary and
    /// secondaries; the LA-ICP-MS part works it out the same way), or "manual".
    pub excess_source: String,
    pub excess_manual_pct: BTreeMap<String, f64>,
    pub excluded_refs: Vec<String>,
    /// Ratios of single materials left out by hand: "CODE|r76".
    #[serde(default)]
    pub excluded_ratios: Vec<String>,
    pub systematic_source: String,
    pub systematic_pct: f64,
}

impl Default for UpbMineralUncertainty {
    fn default() -> Self {
        UpbMineralUncertainty {
            excess_source: "project".into(),
            excess_manual_pct: RATIOS.iter().map(|r| (r.to_string(), 0.0)).collect(),
            excluded_refs: Vec::new(),
            excluded_ratios: Vec::new(),
            systematic_source: "log".into(),
            systematic_pct: DEFAULT_SYSTEMATIC_PCT,
        }
    }
}

fn ratio_pairs(row: &Row) -> BTreeMap<&'static str, (f64, f64)> {
    let p = tw_pair(row);
    let mut out = BTreeMap::new();
    for (k, v, s) in [("r86", p.x, p.sx), ("r76", p.y, p.sy), ("r82", n(row, "UPb_r82"), n(row, "UPb_r82_2s"))] {
        if let (Some(v), Some(s)) = (v, s) {
            if v > 0.0 && s > 0.0 {
                out.insert(k, (v, s / 2.0));
            }
        }
    }
    out
}

/// Reference spots enter the excess without a discordance test (their age is
/// known; a young material's imprecise ²⁰⁷Pb/²⁰⁶Pb would otherwise reject them
/// all); imprecise spots stay out. Ratios a material does not reproduce are
/// left out per ratio instead (see `uncertainty::untrusted`).
fn selection_criteria(c: &Criteria) -> Criteria {
    let mut s = c.clone();
    s.threshold_pct = 1e9;
    s.hard_limit_pct = None;
    s.ellipse_test = false;
    s
}

/// Why a young material's ²⁰⁷Pb/²⁰⁶Pb is not used for the excess or the systematic term.
pub const YOUNG_PB_PB: &str = "younger than 1000 Ma: its ²⁰⁷Pb/²⁰⁶Pb (Pb–Pb age) is too imprecise to be trusted";

/// Median ²⁰⁶Pb/²³⁸U age of a reference material's spots.
fn material_age_ma(rows: &[Row], mineral: &str, code: &str) -> Option<f64> {
    let mut a: Vec<f64> = rows
        .iter()
        .filter(|r| truthy(r, "Is_Reference") && text(r, "Reference") == code && mineral_of(r) == mineral)
        .filter_map(|r| n(r, "UPb_t68_Ma"))
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    a.sort_by(|x, y| x.total_cmp(y));
    a.get(a.len() / 2).copied()
}

/// Is this ratio of this material left out by hand ("CODE|r76")?
pub fn ratio_excluded(excluded_ratios: &[String], code: &str, ratio: &str) -> bool {
    excluded_ratios.iter().any(|x| x == &format!("{code}|{ratio}"))
}

type RGroups = Vec<((String, String), Vec<BTreeMap<&'static str, (f64, f64)>>)>;

fn ref_groups(rows: &[Row], mineral: &str, c: &Criteria, session: Option<&str>) -> RGroups {
    let refs: Vec<&Row> =
        rows.iter().filter(|r| truthy(r, "Is_Reference") && truthy(r, "Reference") && mineral_of(r) == mineral).collect();
    let sc = selection_criteria(c);
    let mut out: RGroups = Vec::new();
    for i in unique_spots(&refs) {
        let r = refs[i];
        // results of the LA-ICP-MS part carry the session's excess from there
        if truthy(r, "UPb_Laser_Excess") {
            continue;
        }
        let s = upb_session(r);
        if session.is_some_and(|x| x != s) {
            continue;
        }
        let mut an = r.clone();
        set(&mut an, CONC_DIST_POINT_FIELD, blank());
        if upb_rejection(&an, &sc).is_some() {
            continue;
        }
        let pairs = ratio_pairs(r);
        if pairs.is_empty() {
            continue;
        }
        let key = (text(r, "Reference"), s);
        match out.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(pairs),
            None => out.push((key, vec![pairs])),
        }
    }
    out
}

fn per_ratio(groups: &[&Vec<BTreeMap<&'static str, (f64, f64)>>], ratio: &str) -> Vec<Vec<(f64, f64)>> {
    groups.iter().map(|g| g.iter().filter_map(|p| p.get(ratio).copied()).collect()).collect()
}

fn pooled(groups: &[&Vec<BTreeMap<&'static str, (f64, f64)>>], ratio: &str) -> (Option<f64>, usize) {
    let usable: Vec<Vec<(f64, f64)>> = per_ratio(groups, ratio).into_iter().filter(|g| g.len() >= 2).collect();
    let dof: usize = usable.iter().map(|g| g.len() - 1).sum();
    if dof < MIN_DOF_FOR_POOLED_EXCESS {
        return (None, dof);
    }
    (Some(200.0 * excess_for_unit_mswd(&usable)), dof)
}

#[derive(Clone, Debug, Default)]
pub struct RatioMaterial {
    pub reference: String,
    pub n: usize,
    pub sessions: usize,
    pub included: bool,
    /// ratio -> (MSWD, excess 2s %)
    pub ratios: BTreeMap<String, (f64, f64)>,
    /// ratio -> why it is left out of the pooled excess
    pub left_out: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct RatioExcessTable {
    pub materials: Vec<RatioMaterial>,
    pub pooled: BTreeMap<String, Option<f64>>,
    pub dof: BTreeMap<String, usize>,
    pub n: usize,
}

pub fn ratio_excess_table(rows: &[Row], mineral: &str, excluded: &[String], excluded_ratios: &[String], c: &Criteria, session: Option<&str>) -> RatioExcessTable {
    let groups = ref_groups(rows, mineral, c, session);
    let mut by_ref: Vec<(String, Vec<&Vec<BTreeMap<&'static str, (f64, f64)>>>)> = Vec::new();
    for ((code, _), g) in &groups {
        match by_ref.iter_mut().find(|x| &x.0 == code) {
            Some(x) => x.1.push(g),
            None => by_ref.push((code.clone(), vec![g])),
        }
    }
    by_ref.sort_by(|a, b| {
        let na: usize = a.1.iter().map(|g| g.len()).sum();
        let nb: usize = b.1.iter().map(|g| g.len()).sum();
        nb.cmp(&na).then(a.0.cmp(&b.0))
    });
    let mut t = RatioExcessTable::default();
    for (code, gs) in &by_ref {
        let mut m = RatioMaterial {
            reference: code.clone(),
            n: gs.iter().map(|g| g.len()).sum(),
            sessions: gs.len(),
            included: !excluded.contains(code),
            ratios: BTreeMap::new(),
            left_out: BTreeMap::new(),
        };
        for r in RATIOS {
            let g2: Vec<Vec<(f64, f64)>> = per_ratio(gs, r).into_iter().filter(|g| g.len() >= 2).collect();
            if !g2.is_empty() {
                let mswd = {
                    let mut ss = 0.0;
                    let mut dof = 0;
                    for g in &g2 {
                        let w: Vec<f64> = g.iter().map(|(_, s)| 1.0 / (s * s)).collect();
                        let sw: f64 = w.iter().sum();
                        let mm = w.iter().zip(g).map(|(wi, (a, _))| wi * a).sum::<f64>() / sw;
                        ss += w.iter().zip(g).map(|(wi, (a, _))| wi * (a - mm).powi(2)).sum::<f64>();
                        dof += g.len() - 1;
                    }
                    ss / dof as f64
                };
                m.ratios.insert(r.into(), (mswd, 200.0 * excess_for_unit_mswd(&g2)));
            }
        }
        t.materials.push(m);
    }
    // per ratio: the materials left out by hand, and the ²⁰⁷Pb/²⁰⁶Pb of young ones
    // (the material with the most spots plays the primary's part and always stays)
    let young: Vec<String> = by_ref
        .iter()
        .skip(1)
        .filter(|(code, _)| material_age_ma(rows, mineral, code).is_some_and(|a| a < T76_PREFERRED_ABOVE_MA))
        .map(|(code, _)| code.clone())
        .collect();
    for r in RATIOS {
        for m in t.materials.iter_mut() {
            if !m.included {
                continue;
            }
            if ratio_excluded(excluded_ratios, &m.reference, r) {
                m.left_out.insert(r.into(), "left out by hand".into());
            } else if r == "r76" && young.contains(&m.reference) {
                m.left_out.insert(r.into(), YOUNG_PB_PB.into());
            }
        }
        let left: Vec<&String> = t.materials.iter().filter(|m| m.left_out.contains_key(r)).map(|m| &m.reference).collect();
        let used: Vec<&Vec<BTreeMap<&'static str, (f64, f64)>>> =
            groups.iter().filter(|((c, _), _)| !excluded.contains(c) && !left.contains(&c)).map(|(_, g)| g).collect();
        let (p, d) = pooled(&used, r);
        t.pooled.insert(r.into(), p);
        t.dof.insert(r.into(), d);
    }
    t.n = groups.iter().filter(|((c, _), _)| !excluded.contains(c)).map(|(_, g)| g.len()).sum();
    t
}

#[derive(Clone, Debug, Default)]
pub struct UpbSessionExcess {
    pub session: String,
    pub n_spots: usize,
    pub n_ref: usize,
    /// ratio -> (own, pct, source)
    pub ratios: BTreeMap<String, (Option<f64>, f64, String)>,
}

pub fn session_ratio_excess(rows: &[Row], mineral: &str, s: &UpbMineralUncertainty, c: &Criteria) -> Vec<UpbSessionExcess> {
    let use_project = s.excess_source == "project";
    let mut counts: BTreeMap<(bool, String), usize> = BTreeMap::new();
    for r in rows {
        // results of the LA-ICP-MS part carry their session's excess from there
        if has_upb(r) && mineral_of(r) == mineral && !truthy(r, "UPb_Laser_Excess") {
            let sn = upb_session(r);
            *counts.entry((sn.is_empty(), sn)).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for ((_, name), cnt) in counts {
        let own = if use_project { Some(ratio_excess_table(rows, mineral, &s.excluded_refs, &s.excluded_ratios, c, Some(&name))) } else { None };
        let mut e = UpbSessionExcess { session: name.clone(), n_spots: cnt, n_ref: own.as_ref().map(|o| o.n).unwrap_or(0), ratios: BTreeMap::new() };
        for r in RATIOS {
            let own_pct = own.as_ref().and_then(|o| o.pooled.get(r).copied().flatten());
            // the session's own reference spots, else the manual value
            let (pct, src) = if let Some(p) = own_pct { (p, "from its reference spots") } else { (s.excess_manual_pct.get(r).copied().unwrap_or(0.0), "manual") };
            e.ratios.insert(r.into(), (own_pct, pct, src.into()));
        }
        out.push(e);
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct UpbMineralModel {
    pub settings: UpbMineralUncertainty,
    pub sessions: Vec<UpbSessionExcess>,
    pub excess_by_session: BTreeMap<String, BTreeMap<String, f64>>,
}

pub fn resolve_upb_uncertainty(rows: &[Row], settings: &BTreeMap<String, UpbMineralUncertainty>, c: &Criteria) -> BTreeMap<String, UpbMineralModel> {
    let mut out = BTreeMap::new();
    for m in MINERAL_NAMES {
        let s = settings.get(m).cloned().unwrap_or_default();
        let sessions = session_ratio_excess(rows, m, &s, c);
        let ebs = sessions.iter().map(|e| (e.session.clone(), e.ratios.iter().map(|(k, v)| (k.clone(), v.1)).collect())).collect();
        out.insert(m.to_string(), UpbMineralModel { settings: s, sessions, excess_by_session: ebs });
    }
    out
}

/// (age, 2σ) in Ma of type `t` at full precision, on analytical + excess (the
/// stored ages are rounded for display; totals are built from these, so they
/// equal the LA-ICP-MS part's).
pub fn full_age_point(row: &Row, t: &str, c: &DecayConstants) -> Option<(f64, f64)> {
    let p = tw_pair_point(row);
    let r82 = n(row, "UPb_r82");
    let s82 = n(row, "UPb_r82_2s_point").or_else(|| n(row, "UPb_r82_2s"));
    let mut copy = Row::new();
    for (k, v) in [("UPb_r86", p.x), ("UPb_r86_2s", p.sx), ("UPb_r76", p.y), ("UPb_r76_2s", p.sy), ("UPb_rho_TW", p.rho), ("UPb_r82", r82), ("UPb_r82_2s", s82)] {
        if let Some(v) = v {
            set(&mut copy, k, jnum(v));
        }
    }
    if !has_upb(&copy) {
        return None;
    }
    let f: BTreeMap<String, Value> = compute_upb_fields(&copy, c, 12).into_iter().collect();
    let age = f.get(&format!("UPb_{t}_Ma")).and_then(|v| v.as_f64())?;
    let s2 = f.get(&format!("UPb_{t}_2s_Ma")).and_then(|v| v.as_f64())?;
    Some((age, s2))
}

pub fn point_fields(row: &Row, excess: &BTreeMap<String, f64>, c: &DecayConstants, dp: usize) -> Row {
    let p = tw_pair(row);
    let (r82, s82) = (n(row, "UPb_r82"), n(row, "UPb_r82_2s"));
    let ex = |r: &str| excess.get(r).copied().unwrap_or(0.0);
    let inflate = |v: Option<f64>, s: Option<f64>, r: &str| -> Option<f64> { Some((s? * s? + (ex(r) / 100.0 * v?).powi(2)).sqrt()) };
    let (px, py, p82) = (inflate(p.x, p.sx, "r86"), inflate(p.y, p.sy, "r76"), inflate(r82, s82, "r82"));
    let mut prho = p.rho;
    if let (Some(rho), Some(pxv), Some(pyv), Some(sx), Some(sy)) = (p.rho, px, py, p.sx, p.sy) {
        if pxv != 0.0 && pyv != 0.0 {
            prho = Some(rho * sx * sy / (pxv * pyv));
        }
    }
    let mut copy = Row::new();
    for (k, v) in [("UPb_r86", p.x), ("UPb_r86_2s", px), ("UPb_r76", p.y), ("UPb_r76_2s", py), ("UPb_rho_TW", prho), ("UPb_r82", r82), ("UPb_r82_2s", p82)] {
        set(&mut copy, k, v.map(jnum).unwrap_or(Value::Null));
    }
    let ages: BTreeMap<String, Value> = if has_upb(&copy) { compute_upb_fields(&copy, c, dp).into_iter().collect() } else { BTreeMap::new() };
    let mut out = Row::new();
    set(&mut out, "UPb_r86_2s_point", jopt(px));
    set(&mut out, "UPb_r76_2s_point", jopt(py));
    set(&mut out, "UPb_rho_TW_point", jopt(prho));
    set(&mut out, "UPb_r82_2s_point", jopt(p82));
    set(&mut out, CONC_DIST_POINT_FIELD, ages.get(CONC_DIST_FIELD).cloned().unwrap_or_else(blank));
    for t in AGE_TYPES {
        set(&mut out, &format!("UPb_{t}_2s_point_Ma"), ages.get(&format!("UPb_{t}_2s_Ma")).cloned().unwrap_or_else(blank));
    }
    set(&mut out, "UPb_Excess_2s_pct", jstr(RATIOS.iter().map(|r| format!("{:.2}", ex(r))).collect::<Vec<_>>().join(" / ")));
    out
}

pub fn apply_upb_uncertainty_model(rows: &mut [Row], model: &BTreeMap<String, UpbMineralModel>, c: &DecayConstants, dp: usize) -> usize {
    let mut cnt = 0;
    for row in rows.iter_mut() {
        if !has_upb(row) {
            continue;
        }
        // laser results from the LA-ICP-MS part bring their session's excess along
        let excess = if truthy(row, "UPb_Laser_Excess") {
            RATIOS.iter().filter_map(|r| n(row, &format!("UPb_Laser_Excess_{r}")).map(|v| (r.to_string(), v))).collect()
        } else {
            model.get(&mineral_of(row)).and_then(|m| m.excess_by_session.get(&upb_session(row))).cloned().unwrap_or_default()
        };
        let p = tw_pair(row);
        let key = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{dp}",
            RATIOS.iter().map(|r| round_to(excess.get(*r).copied().unwrap_or(0.0), 6)).collect::<Vec<_>>(),
            p.x, p.sx, p.y, p.sy, p.rho,
            text(row, "UPb_r82"), text(row, "UPb_r82_2s"), c.lambda_238, c.lambda_235, c.lambda_232, text(row, CONC_DIST_FIELD)
        );
        if text(row, "UPb_PointKey") == key {
            continue;
        }
        let fields = if !RATIOS.iter().any(|r| excess.get(*r).copied().unwrap_or(0.0) != 0.0) {
            let mut f = Row::new();
            set(&mut f, "UPb_r86_2s_point", jopt(p.sx));
            set(&mut f, "UPb_r76_2s_point", jopt(p.sy));
            set(&mut f, "UPb_rho_TW_point", jopt(p.rho));
            set(&mut f, "UPb_r82_2s_point", jopt(n(row, "UPb_r82_2s").filter(|v| *v != 0.0)));
            set(&mut f, CONC_DIST_POINT_FIELD, row.get(CONC_DIST_FIELD).cloned().unwrap_or_else(blank));
            for t in AGE_TYPES {
                set(&mut f, &format!("UPb_{t}_2s_point_Ma"), row.get(&format!("UPb_{t}_2s_Ma")).cloned().unwrap_or_else(blank));
            }
            set(&mut f, "UPb_Excess_2s_pct", jstr("0.00 / 0.00 / 0.00"));
            f
        } else {
            point_fields(row, &excess, c, dp)
        };
        for (k, v) in fields {
            row.insert(k, v);
        }
        set(row, "UPb_PointKey", jstr(key));
        cnt += 1;
    }
    cnt
}

// ---- U-Pb long-term log --------------------------------------------------------

pub fn upb_log_entries_from_rows(rows: &[Row], session_tag: &str, c: &Criteria, timestamp: &str) -> Vec<Row> {
    let refs: Vec<&Row> = rows.iter().filter(|r| truthy(r, "Is_Reference") && truthy(r, "Reference")).collect();
    let mut out = Vec::new();
    for i in unique_spots(&refs) {
        let r = refs[i];
        if upb_rejection(r, c).is_some() {
            continue;
        }
        let mut e = Row::new();
        set(&mut e, "Timestamp", jstr(timestamp));
        set(&mut e, "Reference", jstr(text(r, "Reference")));
        set(&mut e, "Label", jstr(text(r, "Label")));
        set(&mut e, "Mineral", jstr(mineral_of(r)));
        set(&mut e, "SessionTag", jstr(if session_tag.is_empty() { upb_session(r) } else { session_tag.to_string() }));
        set(&mut e, "Row_ID", jstr(text(r, "Row_ID")));
        let mut any = false;
        for t in AGE_TYPES {
            let a = n(r, &format!("UPb_{t}_Ma"));
            set(&mut e, &format!("{t}_Ma"), jopt(a));
            set(&mut e, &format!("{t}_2s_Ma"), jopt(n(r, &format!("UPb_{t}_2s_Ma"))));
            set(&mut e, &format!("{t}_2s_point_Ma"), jopt(n(r, &format!("UPb_{t}_2s_point_Ma"))));
            any |= a.is_some();
        }
        for d in ["Disc76_pct", "Disc75_pct"] {
            set(&mut e, d, jopt(n(r, &format!("UPb_{d}"))));
        }
        if any {
            out.push(e);
        }
    }
    out
}

fn age_entries(log: &[Row], t: &str, mineral: Option<&str>) -> Vec<Row> {
    let mut out = Vec::new();
    for e in log {
        if mineral.is_some_and(|m| mineral_of(e) != m) {
            continue;
        }
        let age = num(e, &format!("{t}_Ma"));
        let s2 = num(e, &format!("{t}_2s_point_Ma")).filter(|v| *v > 0.0).or_else(|| num(e, &format!("{t}_2s_Ma")));
        let (Some(age), Some(s2)) = (age, s2) else { continue };
        if s2 <= 0.0 {
            continue;
        }
        let mut r = Row::new();
        set(&mut r, "Reference", jstr(text(e, "Reference")));
        set(&mut r, "Mineral", jstr(mineral_of(e)));
        set(&mut r, "SessionTag", jstr(text(e, "SessionTag")));
        set(&mut r, "Age_Ma", jnum(age));
        set(&mut r, "Age_2s_Ma", jnum(s2));
        out.push(r);
    }
    out
}

pub fn upb_long_term_overview(log: &[Row], refs: &Refs, t: &str, mineral: Option<&str>, _dp: usize) -> Vec<Row> {
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for e in age_entries(log, t, mineral) {
        let c = text(&e, "Reference");
        if !c.is_empty() {
            groups.entry(c).or_default().push(e);
        }
    }
    let mut out = Vec::new();
    for (code, es) in groups {
        let ages: Vec<f64> = es.iter().map(|e| num(e, "Age_Ma").unwrap()).collect();
        let nn = ages.len();
        let mut tags: Vec<String> = es.iter().map(|e| text(e, "SessionTag")).collect();
        tags.sort();
        tags.dedup();
        let mut r = Row::new();
        set(&mut r, "Reference", jstr(&code));
        set(&mut r, "N_logged", jnum(nn as f64));
        set(&mut r, "N_sessions", jnum(tags.len() as f64));
        set(&mut r, "Mean_Ma", jnum(mean(&ages)));
        set(&mut r, "SD2_Ma", if nn > 1 { jnum(2.0 * stdev(&ages)) } else { blank() });
        for k in ["WM_Ma", "WM_2s_Ma", "MSWD", "Accepted_Ma", "Accepted_2s_Ma", "Diff_Ma", "Diff_pct", "Within"] {
            set(&mut r, k, blank());
        }
        let mut wm = None;
        if nn >= 2 {
            let s: Vec<f64> = es.iter().map(|e| num(e, "Age_2s_Ma").unwrap() / 2.0).collect();
            if let Some(f) = crate::stats::weighted_mean(&ages, &s, false, 2.0) {
                set(&mut r, "WM_Ma", jnum(f.mean));
                set(&mut r, "WM_2s_Ma", jnum(f.expanded_2s));
                set(&mut r, "MSWD", jnum(f.mswd));
                wm = Some((f.mean, f.expanded_2s));
            }
        }
        let (acc, acc2) = accepted_upb_age(refs, &code, t);
        if let Some(acc) = acc {
            set(&mut r, "Accepted_Ma", jnum(acc));
            set(&mut r, "Accepted_2s_Ma", jopt(acc2));
            if let Some((w, w2)) = wm {
                let d = w - acc;
                set(&mut r, "Diff_Ma", jnum(d));
                set(&mut r, "Diff_pct", if acc != 0.0 { jnum(100.0 * d / acc) } else { blank() });
                set(&mut r, "Within", Value::Bool(d.abs() <= (w2 * w2 + acc2.unwrap_or(0.0).powi(2)).sqrt()));
            }
        }
        out.push(r);
    }
    out
}

pub fn between_session_upb_systematic(log: &[Row], mineral: &str, t: &str, excluded: &[String]) -> (Option<f64>, Vec<crate::uncertainty::MaterialBetween>) {
    let mut ex = excluded.to_vec();
    let mut young: Vec<String> = Vec::new();
    if t == "t76" {
        // Pb–Pb ages of young reference materials are too imprecise to be trusted
        let mut ages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for e in log.iter().filter(|e| mineral_of(e) == mineral) {
            if let Some(a) = n(e, "t68_Ma") {
                ages.entry(text(e, "Reference")).or_default().push(a);
            }
        }
        for (code, mut a) in ages {
            a.sort_by(|x, y| x.total_cmp(y));
            if a.get(a.len() / 2).is_some_and(|m| *m < T76_PREFERRED_ABOVE_MA) && !ex.contains(&code) {
                ex.push(code.clone());
                young.push(code);
            }
        }
    }
    let (p, mut mats) = between_session_systematic(&age_entries(log, t, None), mineral, &ex, Some(0.0), &BTreeMap::new());
    for code in young {
        mats.push(crate::uncertainty::MaterialBetween { reference: code, left_out: Some(YOUNG_PB_PB.into()), ..Default::default() });
    }
    (p, mats)
}

/// The ratios behind an age type ("t68" <- "r86", ...).
pub fn ratios_of_age(t: &str) -> &'static [&'static str] {
    match t {
        "t68" => &["r86"],
        "t76" => &["r76"],
        "t75" => &["r86", "r76", "r75"],
        "t82" => &["r82"],
        _ => &[],
    }
}

/// Materials left out of the systematic term of age type `t`: whole materials,
/// and those whose ratio behind `t` is left out by hand.
pub fn excluded_for_age(s: &UpbMineralUncertainty, t: &str) -> Vec<String> {
    let mut v = s.excluded_refs.clone();
    for x in &s.excluded_ratios {
        if let Some((code, r)) = x.split_once('|') {
            if ratios_of_age(t).contains(&r) && !v.iter().any(|c| c == code) {
                v.push(code.to_string());
            }
        }
    }
    v
}

/// The systematic term of U–Pb means, per mineral and age type: from the
/// long-term log (≥ 3 sessions) or manual; `decay` is set when the
/// decay-constant uncertainties are to be added.
#[derive(Clone, Debug, Default)]
pub struct UpbSystematic {
    /// mineral -> age type -> (pct, source)
    pub by_mineral: BTreeMap<String, BTreeMap<String, (f64, String)>>,
    pub decay: Option<DecayConstants>,
}

pub fn resolve_upb_systematic(settings: &BTreeMap<String, UpbMineralUncertainty>, log: &[Row]) -> UpbSystematic {
    let mut out = UpbSystematic::default();
    for m in MINERAL_NAMES {
        let s = settings.get(m).cloned().unwrap_or_default();
        let mut per = BTreeMap::new();
        for t in AGE_TYPES {
            let (mut pct, mut src) = (s.systematic_pct, "manual".to_string());
            if s.systematic_source == "log" {
                match between_session_upb_systematic(log, m, t, &excluded_for_age(&s, t)) {
                    (Some(p), _) => {
                        pct = p;
                        src = "long-term log".into();
                    }
                    _ => src = format!("manual, the log has < {} sessions", crate::uncertainty::MIN_SESSIONS_FOR_LOG_SYSTEMATIC),
                }
            }
            per.insert(t.to_string(), (pct, src));
        }
        out.by_mineral.insert(m.to_string(), per);
    }
    out
}

/// Field of a laser row: the primary's accepted-value uncertainty as a
/// relative 2σ (%) of the age of type `t` (written when laser results are sent).
pub fn primary_rm_field(t: &str) -> String {
    format!("UPb_{t}_PrimaryRM_2s_pct")
}

/// Relative 2σ (%) of an age of type `t` (at `age_ma`) from the decay-constant
/// uncertainties. ²³²Th has no stated uncertainty in the settings.
pub fn decay_rel_2s_pct(t: &str, age_ma: f64, c: &DecayConstants) -> f64 {
    let (r8, r5) = (2.0 * c.lambda_238_rel_1s_pct, 2.0 * c.lambda_235_rel_1s_pct);
    match t {
        "t68" | "concordia" => r8,
        "t75" => r5,
        "t76" => {
            // numerically: the 207/206 age moves with either constant
            let Some(r76) = crate::upb::r76_from_age(age_ma * 1e6, c) else { return 0.0 };
            let shift = |f8: f64, f5: f64| -> f64 {
                let mut d = c.clone();
                d.lambda_238 *= 1.0 + f8;
                d.lambda_235 *= 1.0 + f5;
                crate::upb::age_76(r76, &d).map(|a| (a / 1e6 / age_ma - 1.0).abs() * 100.0).unwrap_or(0.0)
            };
            let a = shift(r8 / 100.0, 0.0);
            let b = shift(0.0, r5 / 100.0);
            (a * a + b * b).sqrt()
        }
        _ => 0.0,
    }
}

pub fn group_mineral(rows: &[&Row]) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for r in rows {
        if AGE_TYPES.iter().any(|t| n(r, &format!("UPb_{t}_Ma")).is_some()) {
            let m = mineral_of(r);
            if !counts.contains_key(&m) {
                order.push(m.clone());
            }
            *counts.entry(m).or_default() += 1;
        }
    }
    let mut best: Option<(String, usize)> = None;
    for m in order {
        let c = counts[&m];
        if best.as_ref().is_none_or(|b| c > b.1) {
            best = Some((m, c));
        }
    }
    best.map(|b| b.0).unwrap_or_else(|| DEFAULT_MINERAL.into())
}

pub fn systematic_for(resolved: &UpbSystematic, rows: &[&Row], t: &str, manual: f64) -> (f64, String) {
    let tt = if t == "concordia" { "t68" } else { t };
    let (base, mut src) = match resolved.by_mineral.get(&group_mineral(rows)).and_then(|m| m.get(tt)) {
        Some((p, s)) => (*p, s.clone()),
        None => (manual, "manual".into()),
    };
    let mut total2 = base * base;
    // the primary reference material's accepted value (laser rows carry it)
    let mut rm: Vec<f64> = rows.iter().filter_map(|r| n(r, &primary_rm_field(tt))).filter(|v| v.is_finite() && *v > 0.0).collect();
    if !rm.is_empty() {
        rm.sort_by(|a, b| a.total_cmp(b));
        let p = rm[rm.len() / 2];
        total2 += p * p;
        src.push_str(&format!(" + primary RM {p:.2} %"));
    }
    if let Some(c) = &resolved.decay {
        let mut ages: Vec<f64> = rows.iter().filter_map(|r| n(r, &format!("UPb_{tt}_Ma"))).filter(|v| v.is_finite() && *v > 0.0).collect();
        if !ages.is_empty() {
            ages.sort_by(|a, b| a.total_cmp(b));
            let p = decay_rel_2s_pct(tt, ages[ages.len() / 2], c);
            if p > 0.0 {
                total2 += p * p;
                src.push_str(&format!(" + decay constants {p:.2} %"));
            }
        }
    }
    (total2.sqrt(), src)
}
