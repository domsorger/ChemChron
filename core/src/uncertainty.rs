// SPDX-License-Identifier: GPL-3.0-or-later
//! Two-component uncertainty model of chemical ages: a random excess per
//! analysis (from the session's reference materials) and a systematic term
//! per session (manual, or from the long-term log).

use crate::constants::{DecimalPlaces, MINERAL_NAMES};
use crate::reference::{recompute_ref_fields_for_row, Refs};
use crate::row::*;
use crate::sessions::epma_session;
use crate::stats::{inflate_for_overdispersion, mineral_of, usable_for_age_statistics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_EXCESS_REL_2S_PCT: f64 = 2.6;
pub const DEFAULT_SYSTEMATIC_REL_2S_PCT: f64 = 2.0;
pub const MIN_DOF_FOR_POOLED_EXCESS: usize = 3;
pub const MIN_ANALYSES_PER_LOGGED_SESSION: usize = 3;
/// The long-term log sets the systematic term once it holds this many sessions
/// (with enough reference analyses); before that the manual value is used.
pub const MIN_SESSIONS_FOR_LOG_SYSTEMATIC: usize = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MineralUncertainty {
    /// "project" (from reference materials) or "manual".
    pub excess_source: String,
    pub excess_manual_pct: f64,
    pub excess_excluded_refs: Vec<String>,
    /// "log" (from the long-term log once it has enough sessions, else the
    /// manual value) or "manual".
    pub systematic_source: String,
    /// Manual systematic term, 2s %.
    pub manual_override_pct: f64,
}

impl Default for MineralUncertainty {
    fn default() -> Self {
        MineralUncertainty {
            excess_source: "project".into(),
            excess_manual_pct: DEFAULT_EXCESS_REL_2S_PCT,
            excess_excluded_refs: Vec::new(),
            systematic_source: "log".into(),
            manual_override_pct: DEFAULT_SYSTEMATIC_REL_2S_PCT,
        }
    }
}

type Groups = Vec<Vec<(f64, f64)>>;

fn pooled_mswd(groups: &Groups, rel_1s: f64) -> f64 {
    let (mut ss, mut dof) = (0.0, 0usize);
    for pairs in groups {
        let w: Vec<f64> = pairs.iter().map(|(a, s)| 1.0 / (s * s + (rel_1s * a).powi(2))).collect();
        let sw: f64 = w.iter().sum();
        let m = w.iter().zip(pairs).map(|(wi, (a, _))| wi * a).sum::<f64>() / sw;
        ss += w.iter().zip(pairs).map(|(wi, (a, _))| wi * (a - m).powi(2)).sum::<f64>();
        dof += pairs.len().saturating_sub(1);
    }
    if dof > 0 {
        ss / dof as f64
    } else {
        f64::NAN
    }
}

/// Relative 1-sigma excess that brings the pooled MSWD to 1.
pub fn excess_for_unit_mswd(groups: &Groups) -> f64 {
    if !(pooled_mswd(groups, 0.0) > 1.0) {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if pooled_mswd(groups, mid) > 1.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

fn reference_groups(rows: &[Row], mineral: &str, session: Option<&str>) -> Vec<((String, String), Vec<(f64, f64)>)> {
    let mut out: Vec<((String, String), Vec<(f64, f64)>)> = Vec::new();
    for row in rows {
        if !truthy(row, "Is_Reference") || mineral_of(row) != mineral || !usable_for_age_statistics(row) {
            continue;
        }
        let sess = epma_session(row);
        if session.is_some_and(|s| s != sess) {
            continue;
        }
        let code = text(row, "Reference");
        let (Some(age), Some(a2s)) = (num(row, "Age_Ma"), num(row, "Age_2s_Ma")) else { continue };
        if code.is_empty() || a2s <= 0.0 {
            continue;
        }
        let key = (code, sess);
        match out.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push((age, a2s / 2.0)),
            None => out.push((key, vec![(age, a2s / 2.0)])),
        }
    }
    out
}

fn pooled_excess(groups: &Groups) -> (Option<f64>, usize) {
    let usable: Groups = groups.iter().filter(|g| g.len() >= 2).cloned().collect();
    let dof: usize = usable.iter().map(|g| g.len() - 1).sum();
    if dof < MIN_DOF_FOR_POOLED_EXCESS {
        return (None, dof);
    }
    (Some(200.0 * excess_for_unit_mswd(&usable)), dof)
}

#[derive(Clone, Debug, Default)]
pub struct MaterialExcess {
    pub reference: String,
    pub n: usize,
    pub sessions: usize,
    pub mswd: Option<f64>,
    pub excess_2s_pct: Option<f64>,
    pub included: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ExcessTable {
    pub materials: Vec<MaterialExcess>,
    pub pooled_pct: Option<f64>,
    pub dof: usize,
    pub n: usize,
    pub n_materials: usize,
}

pub fn reference_excess_table(rows: &[Row], mineral: &str, excluded: &[String], session: Option<&str>) -> ExcessTable {
    let groups = reference_groups(rows, mineral, session);
    let mut by_ref: Vec<(String, Groups)> = Vec::new();
    for ((code, _), pairs) in &groups {
        match by_ref.iter_mut().find(|g| &g.0 == code) {
            Some(g) => g.1.push(pairs.clone()),
            None => by_ref.push((code.clone(), vec![pairs.clone()])),
        }
    }
    by_ref.sort_by(|a, b| {
        let na: usize = a.1.iter().map(|g| g.len()).sum();
        let nb: usize = b.1.iter().map(|g| g.len()).sum();
        nb.cmp(&na).then(a.0.cmp(&b.0))
    });
    let mut materials = Vec::new();
    for (code, gs) in &by_ref {
        let rg: Groups = gs.iter().filter(|g| g.len() >= 2).cloned().collect();
        materials.push(MaterialExcess {
            reference: code.clone(),
            n: gs.iter().map(|g| g.len()).sum(),
            sessions: gs.len(),
            mswd: if rg.is_empty() { None } else { Some(pooled_mswd(&rg, 0.0)) },
            excess_2s_pct: if rg.is_empty() { None } else { Some(200.0 * excess_for_unit_mswd(&rg)) },
            included: !excluded.contains(code),
        });
    }
    let used: Groups =
        groups.iter().filter(|((c, _), g)| !excluded.contains(c) && g.len() >= 2).map(|(_, g)| g.clone()).collect();
    let (pooled, dof) = pooled_excess(&used);
    let mut mats: Vec<&String> =
        groups.iter().filter(|((c, _), g)| !excluded.contains(c) && g.len() >= 2).map(|((c, _), _)| c).collect();
    mats.sort();
    mats.dedup();
    ExcessTable { materials, pooled_pct: pooled, dof, n: used.iter().map(|g| g.len()).sum(), n_materials: mats.len() }
}

#[derive(Clone, Debug)]
pub struct SessionExcess {
    pub session: String,
    pub n_analyses: usize,
    pub n_ref: usize,
    pub dof: usize,
    pub session_pct: Option<f64>,
    pub pct: f64,
    /// "session", "project" or "manual".
    pub source: String,
}

pub fn session_excess_table(rows: &[Row], mineral: &str, excluded: &[String], manual_pct: f64, use_project: bool) -> Vec<SessionExcess> {
    let project = if use_project { reference_excess_table(rows, mineral, excluded, None).pooled_pct } else { None };
    let mut sessions: BTreeMap<(bool, String), usize> = BTreeMap::new();
    for row in rows {
        if mineral_of(row) == mineral && !truthy(row, "No_EPMA") {
            let s = epma_session(row);
            *sessions.entry((s.is_empty(), s)).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for ((_, name), n) in sessions {
        let own = if use_project { Some(reference_excess_table(rows, mineral, excluded, Some(&name))) } else { None };
        let own_pct = own.as_ref().and_then(|o| o.pooled_pct);
        let (pct, src) = if let Some(p) = own_pct {
            (p, "session")
        } else if let Some(p) = project {
            (p, "project")
        } else {
            (manual_pct, "manual")
        };
        out.push(SessionExcess {
            session: name,
            n_analyses: n,
            n_ref: own.as_ref().map(|o| o.n).unwrap_or(0),
            dof: own.as_ref().map(|o| o.dof).unwrap_or(0),
            session_pct: own_pct,
            pct,
            source: src.into(),
        });
    }
    out
}

fn session_mean(entries: &[&Row], excess_pct: Option<f64>) -> (f64, f64) {
    let ages: Vec<f64> = entries.iter().map(|e| num_loose(e, "Age_Ma").unwrap_or(f64::NAN)).collect();
    let a2s: Vec<Option<f64>> = entries.iter().map(|e| num(e, "Age_2s_Ma")).collect();
    let n = ages.len();
    if a2s.iter().all(|v| v.is_some_and(|x| x > 0.0)) {
        let rel = excess_pct.unwrap_or(0.0) / 200.0;
        let w: Vec<f64> =
            a2s.iter().zip(&ages).map(|(v, a)| 1.0 / ((v.unwrap() / 2.0).powi(2) + (rel * a).powi(2))).collect();
        let sw: f64 = w.iter().sum();
        let m = w.iter().zip(&ages).map(|(wi, a)| wi * a).sum::<f64>() / sw;
        let mut se = 1.0 / sw.sqrt();
        let mswd = w.iter().zip(&ages).map(|(wi, a)| wi * (a - m).powi(2)).sum::<f64>() / (n as f64 - 1.0);
        if inflate_for_overdispersion(Some(mswd), n as i64 - 1).0 {
            se *= mswd.sqrt();
        }
        return (m, se);
    }
    (mean(&ages), stdev(&ages) / (n as f64).sqrt())
}

#[derive(Clone, Debug, Default)]
pub struct MaterialBetween {
    pub reference: String,
    pub sessions: usize,
    pub usable_sessions: usize,
    pub between_2s_pct: Option<f64>,
    /// Why it was left out of the pooled term (None: used).
    pub left_out: Option<String>,
}

pub fn between_session_systematic(
    log_rows: &[Row],
    mineral: &str,
    excluded: &[String],
    excess_pct: Option<f64>,
    excess_by_session: &BTreeMap<String, f64>,
) -> (Option<f64>, Vec<MaterialBetween>) {
    let mut by_ref: BTreeMap<String, Vec<(String, Vec<&Row>)>> = BTreeMap::new();
    for e in log_rows {
        if mineral_of(e) != mineral {
            continue;
        }
        let code = text(e, "Reference");
        if code.is_empty() || excluded.contains(&code) || num(e, "Age_Ma").is_none() {
            continue;
        }
        let tag = text(e, "SessionTag");
        let list = by_ref.entry(code).or_default();
        match list.iter_mut().find(|s| s.0 == tag) {
            Some(s) => s.1.push(e),
            None => list.push((tag, vec![e])),
        }
    }
    let mut materials = Vec::new();
    let mut candidates: Vec<(usize, Vec<String>, Vec<(f64, f64)>)> = Vec::new();
    for (code, sessions) in by_ref {
        let good: Vec<&(String, Vec<&Row>)> = sessions.iter().filter(|s| s.1.len() >= MIN_ANALYSES_PER_LOGGED_SESSION).collect();
        let mut info = MaterialBetween { reference: code, sessions: sessions.len(), usable_sessions: good.len(), between_2s_pct: None, left_out: None };
        if good.len() >= 2 {
            let pairs: Vec<(f64, f64)> = good
                .iter()
                .map(|(tag, e)| session_mean(e, excess_by_session.get(tag).copied().or(excess_pct)))
                .collect();
            info.between_2s_pct = Some(200.0 * excess_for_unit_mswd(&vec![pairs.clone()]));
            candidates.push((materials.len(), good.iter().map(|s| s.0.clone()).collect(), pairs));
        }
        materials.push(info);
    }
    let mut groups: Groups = Vec::new();
    let mut usable_tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (_, tags, pairs) in candidates {
        usable_tags.extend(tags);
        groups.push(pairs);
    }
    let pct = if groups.is_empty() || usable_tags.len() < MIN_SESSIONS_FOR_LOG_SYSTEMATIC { None } else { Some(200.0 * excess_for_unit_mswd(&groups)) };
    (pct, materials)
}

#[derive(Clone, Debug, Default)]
pub struct MineralModel {
    pub excess_pct: f64,
    pub excess_source: String,
    /// EPMA session -> (pct, source)
    pub excess_by_session: BTreeMap<String, (f64, String)>,
    pub systematic_pct: f64,
    pub systematic_source: String,
}

pub type Model = BTreeMap<String, MineralModel>;

pub fn resolve_uncertainty_model(rows: &[Row], log_rows: &[Row], settings: &BTreeMap<String, MineralUncertainty>) -> Model {
    let mut model = Model::new();
    for mineral in MINERAL_NAMES {
        let s = settings.get(mineral).cloned().unwrap_or_default();
        let (mut excess, mut excess_src) = (s.excess_manual_pct, "manual");
        let use_project = s.excess_source == "project";
        if use_project {
            if let Some(p) = reference_excess_table(rows, mineral, &s.excess_excluded_refs, None).pooled_pct {
                excess = p;
                excess_src = "project";
            }
        }
        let by_session: BTreeMap<String, (f64, String)> =
            session_excess_table(rows, mineral, &s.excess_excluded_refs, s.excess_manual_pct, use_project)
                .into_iter()
                .map(|e| (e.session, (e.pct, e.source)))
                .collect();
        let (mut sys, mut sys_src) = (s.manual_override_pct, "manual".to_string());
        if s.systematic_source == "log" {
            let ebs: BTreeMap<String, f64> =
                by_session.iter().filter(|(k, _)| !k.is_empty()).map(|(k, v)| (k.clone(), v.0)).collect();
            match between_session_systematic(log_rows, mineral, &s.excess_excluded_refs, Some(excess), &ebs) {
                (Some(p), _) => {
                    sys = p;
                    sys_src = "long-term log".into();
                }
                _ => sys_src = format!("manual, the log has < {MIN_SESSIONS_FOR_LOG_SYSTEMATIC} sessions"),
            }
        }
        model.insert(
            mineral.to_string(),
            MineralModel {
                excess_pct: excess,
                excess_source: excess_src.into(),
                excess_by_session: by_session,
                systematic_pct: sys,
                systematic_source: sys_src,
            },
        );
    }
    model
}

pub const UNCERTAINTY_FIELDS: [&str; 6] = [
    "Age_2s_Ma_point",
    "Age_Rel_2s_pct_point",
    "Age_2s_Ma_total",
    "Age_Rel_2s_pct_total",
    "Excess_2s_rel_pct",
    "LongTerm_2s_rel_pct",
];

pub fn uncertainty_fields(age: Option<f64>, a2s: Option<f64>, excess: Option<f64>, sys: Option<f64>, _dp: &DecimalPlaces) -> Vec<(&'static str, serde_json::Value)> {
    let mut out = vec![
        ("Age_2s_Ma_point", blank()),
        ("Age_Rel_2s_pct_point", blank()),
        ("Age_2s_Ma_total", blank()),
        ("Age_Rel_2s_pct_total", blank()),
        ("Excess_2s_rel_pct", jopt(excess)),
        ("LongTerm_2s_rel_pct", jopt(sys)),
    ];
    let (Some(age), Some(a2s)) = (age, a2s) else { return out };
    let exc = excess.unwrap_or(0.0) / 100.0 * age;
    let sy = sys.unwrap_or(0.0) / 100.0 * age;
    let point = (a2s * a2s + exc * exc).sqrt();
    let total = (point * point + sy * sy).sqrt();
    out[0].1 = jnum(point);
    out[1].1 = if age != 0.0 { jnum(100.0 * point / age) } else { blank() };
    out[2].1 = jnum(total);
    out[3].1 = if age != 0.0 { jnum(100.0 * total / age) } else { blank() };
    out
}

pub fn blank_uncertainty() -> Vec<(&'static str, serde_json::Value)> {
    UNCERTAINTY_FIELDS.iter().map(|f| (*f, blank())).collect()
}

/// Excess in effect for a row under `model`.
pub fn excess_for_row(model: &Model, row: &Row) -> Option<f64> {
    let m = model.get(&mineral_of(row))?;
    Some(m.excess_by_session.get(&epma_session(row)).map(|v| v.0).unwrap_or(m.excess_pct))
}

pub fn apply_uncertainty_model(rows: &mut [Row], model: &Model, refs: &Refs, dp: &DecimalPlaces) -> usize {
    let mut changed = 0;
    for row in rows.iter_mut() {
        let fields = match model.get(&mineral_of(row)) {
            None => blank_uncertainty(),
            Some(m) => {
                let excess = m.excess_by_session.get(&epma_session(row)).map(|v| v.0).unwrap_or(m.excess_pct);
                uncertainty_fields(num(row, "Age_Ma"), num(row, "Age_2s_Ma"), Some(excess), Some(m.systematic_pct), dp)
            }
        };
        if row.get("Age_2s_Ma_total") != Some(&fields[2].1) {
            changed += 1;
        }
        for (k, v) in fields {
            set(row, k, v);
        }
        recompute_ref_fields_for_row(row, refs, dp);
    }
    changed
}
