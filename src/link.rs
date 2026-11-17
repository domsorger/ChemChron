// SPDX-License-Identifier: GPL-3.0-or-later
//! From the Laser part to the analysis table: the reduced U–Pb ratios and
//! trace-element contents of each laser spot, as import records that are
//! linked to EPMA analyses by label (the same way as imported files).

use chemchron_core::row::*;
use chemchron_core::trace::{normalize_label, Entry, TraceImport, TraceRecord};
use chemchron_core::upb::Record;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use chemchron_laser::app::TraceApp;
use chemchron_laser_core::model::{GroupKind, Session};

fn fin(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// Label of a laser spot as it is matched to EPMA analyses (file endings removed).
pub fn spot_label(label: &str) -> String {
    let l = label.trim();
    for ext in [".FIN2", ".fin2", ".FIN", ".fin", ".csv", ".CSV", ".d", ".D"] {
        if let Some(s) = l.strip_suffix(ext) {
            return s.to_string();
        }
    }
    l.to_string()
}

pub struct LaserSpots {
    pub upb: Vec<Record>,
    pub trace: TraceImport,
    pub session_name: String,
    pub n_left_out: usize,
}

/// Every analysis of the laser session (not baselines, not left out).
pub fn laser_spots(app: &TraceApp, include_refs: bool) -> LaserSpots {
    let session = &app.session;
    let red = &app.red;
    let mut upb = Vec::new();
    let mut trace = TraceImport::default();
    let mut n_left_out = 0;
    let te_channels: Vec<(usize, String)> = red
        .te
        .as_ref()
        .map(|t| {
            t.calibrations
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_some())
                .filter_map(|(i, _)| {
                    let name = &session.inputs.channels[i].name;
                    chemchron_core::trace::parse_header(name).map(|h| (i, h.element))
                })
                .collect()
        })
        .unwrap_or_default();
    let lod_method = session.settings.te.lod;
    let mut elements: Vec<String> = Vec::new();
    for (s, r) in session.selections.iter().zip(&red.selections) {
        let Some(group) = session.group_of(s) else { continue };
        if group.kind == GroupKind::Baseline {
            continue;
        }
        if group.kind == GroupKind::ReferenceMaterial && !include_refs {
            continue;
        }
        if s.excluded {
            n_left_out += 1;
            continue;
        }
        let label = spot_label(&s.label);
        let key = normalize_label(&label);
        let gname = if group.kind == GroupKind::Sample { group.name.clone() } else { String::new() };
        if let Some(u) = &r.upb {
            let mut v = Row::new();
            let mut put = |k: &str, x: f64| {
                if let Some(x) = fin(x) {
                    set(&mut v, k, jnum(x));
                }
            };
            // measured, or common-Pb (and 230Th) corrected, as chosen on LA-ICP-MS > Export
            let d = &session.settings.upb.decay;
            let mut r68 = (u.r68.mean, u.r68.prop2s);
            let mut r75 = (u.r75.mean, u.r75.prop2s);
            let mut r82 = u.r82.as_ref().map(|r| (r.mean, r.prop2s));
            let mut note = "measured";
            if app.send_ratios != chemchron_laser::app::SendRatios::Measured {
                if let Some(c) = &u.common {
                    r68 = (c.r68, c.r68_2s);
                    r75 = (c.r75, c.r75_2s);
                    if c.r82.is_finite() {
                        r82 = Some((c.r82, c.r82_2s));
                    }
                    note = "common-Pb corrected";
                    if app.send_ratios == chemchron_laser::app::SendRatios::CommonPbTh230 {
                        if let Some(a) = u.a68_th230.filter(|a| a.age.is_finite()) {
                            let t = a.age * 1e6;
                            let r = (d.l238 * t).exp_m1();
                            let dr = d.l238 * (d.l238 * t).exp() * a.prop2s * 1e6;
                            r68 = (r, dr.abs());
                            note = "common-Pb and 230Th corrected";
                        }
                    }
                }
            }
            // Combined results gets the analytical 2σ and the session's excess
            // apart (it adds the excess itself, as for chemical ages): the
            // excess part (relative 2σ) comes off each 2σ again
            use chemchron_laser_core::drs::upb::RatioKind;
            let ex = |k: RatioKind| red.upb.as_ref().and_then(|m| m.ratio(k)).map_or(0.0, |r| 2.0 * r.excess);
            let base = |(v, s2): (f64, f64), e: f64| (v, (s2 * s2 - (e * v).powi(2)).max(0.0).sqrt());
            let (e68, e75, e76, e82) = (ex(RatioKind::R68), ex(RatioKind::R75), ex(RatioKind::R76), ex(RatioKind::R82));
            let r68 = base(r68, e68);
            let r75 = base(r75, e75);
            let r82 = r82.map(|x| base(x, e82));
            let (r86, r86_2s) = (1.0 / r68.0, r68.1 / (r68.0 * r68.0));
            let r76v = if app.send_ratios == chemchron_laser::app::SendRatios::Measured { u.r76.mean } else { r75.0 / r68.0 / d.u85 };
            let r76_2s = if app.send_ratios == chemchron_laser::app::SendRatios::Measured { base((u.r76.mean, u.r76.prop2s), e76).1 } else { r76v * ((r75.1 / r75.0).powi(2) + (r68.1 / r68.0).powi(2)).sqrt() };
            put("UPb_Laser_Excess_r86", 100.0 * e68);
            put("UPb_Laser_Excess_r76", 100.0 * e76);
            put("UPb_Laser_Excess_r82", 100.0 * e82);
            put("UPb_r86", r86);
            put("UPb_r86_2s", r86_2s);
            put("UPb_r76", r76v);
            put("UPb_r76_2s", r76_2s);
            put("UPb_rho_TW", u.rho_76_38);
            put("UPb_r68", r68.0);
            put("UPb_r68_2s", r68.1);
            put("UPb_r75", r75.0);
            put("UPb_r75_2s", r75.1);
            put("UPb_rho_W", u.rho_68_75);
            if let Some((a, b)) = r82 {
                put("UPb_r82", a);
                put("UPb_r82_2s", b);
            }
            // the primary's accepted-value uncertainty, for the systematic term of means
            for (t, a) in [("t68", Some(u.a68)), ("t75", Some(u.a75)), ("t76", Some(u.a76)), ("t82", u.a82)] {
                if let Some(a) = a.filter(|a| a.age.is_finite() && a.age > 0.0) {
                    let rm = (a.rminc2s.powi(2) - a.prop2s.powi(2)).max(0.0).sqrt();
                    put(&chemchron_core::upbstats::primary_rm_field(t), 100.0 * rm / a.age);
                }
            }
            set(&mut v, "UPb_Ratios", jstr(note));
            // what the spot is in the laser session: a sample, or which reference material
            set(&mut v, "UPb_Laser_Kind", jstr(if group.kind == GroupKind::ReferenceMaterial { "reference" } else { "sample" }));
            if group.kind == GroupKind::ReferenceMaterial {
                if let Some(m) = session.rm_of(group.id) {
                    set(&mut v, "UPb_Laser_RM", jstr(&m.name));
                }
            }
            // the session's excess (primary + secondaries) is already in these 2σ
            set(&mut v, "UPb_Laser_Excess", serde_json::Value::Bool(true));
            if v.contains_key("UPb_r86") || v.contains_key("UPb_r68") || v.contains_key("UPb_r82") {
                upb.push(Record { label: label.clone(), key: key.clone(), values: v, group: gname.clone() });
            }
        }
        if !te_channels.is_empty() {
            let mut values: BTreeMap<String, Entry> = BTreeMap::new();
            for (c, el) in &te_channels {
                let Some(tv) = r.te.get(*c).and_then(|x| x.as_ref()) else { continue };
                if !elements.contains(el) {
                    elements.push(el.clone());
                }
                let e = values.entry(el.clone()).or_default();
                e.lod = fin(tv.lod(lod_method));
                if tv.below_lod {
                    e.bdl = true;
                } else {
                    e.ppm = fin(tv.ppm);
                    e.s2 = fin(tv.prop2s);
                    if e.ppm.is_none_or(|p| p <= 0.0) {
                        e.bdl = true;
                    }
                }
            }
            if !values.is_empty() {
                for (el, e) in &values {
                    if e.bdl {
                        *trace.bdl_counts.entry(el.clone()).or_default() += 1;
                    }
                }
                trace.records.push(TraceRecord { label: label.clone(), key: key.clone(), values });
            }
        }
    }
    trace.elements = chemchron_core::trace::canonical_order(elements);
    LaserSpots { upb, trace, session_name: session.name.clone(), n_left_out }
}

/// The laser session saved inside a ChemChron project, reopened from its raw files.
pub fn load_laser_value(v: &serde_json::Value, project_dir: Option<&Path>) -> Result<(Session, Vec<String>), String> {
    let session: Session = serde_json::from_value(v.clone()).map_err(|e| format!("unreadable laser session: {e}"))?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = project_dir {
        dirs.push(d.to_path_buf());
        if let Some(u) = d.parent() {
            dirs.push(u.to_path_buf());
        }
    }
    let loaded = chemchron_laser_core::project::reopen_session(session, &dirs)?;
    let mut s = loaded.session;
    if !loaded.changed_files.is_empty() {
        let mut inputs = (*s.inputs).clone();
        inputs.warnings.extend(loaded.changed_files.clone());
        s.inputs = std::sync::Arc::new(inputs);
    }
    Ok((s, loaded.changed_files))
}

/// Internal standard per laser spot from the EPMA analysis with exactly the
/// same label: the oxide of the internal-standard element (wt%, the oxide
/// chosen in LA-ICP-MS > Corrections when the EPMA has it), kept as element ppm.
/// Returns a message for the user.
pub fn internal_standard_from_epma(app: &mut crate::app::ChemApp) -> (String, bool) {
    let element = app.laser.is_element();
    if element.is_empty() {
        return ("Choose the internal-standard channel first (LA-ICP-MS > Corrections > Trace elements).".into(), true);
    }
    let mut oxides: Vec<String> = app.data.active.clone();
    oxides.extend(chemchron_core::constants::FULL_REE_OXIDES.iter().map(|s| s.to_string()));
    let wanted = app.laser.is_oxide();
    let oxide = match wanted.filter(|o| oxides.contains(o)) {
        Some(o) => o,
        None => match oxides.into_iter().find(|o| chemchron_core::constants::element_symbol(o) == element) {
            Some(o) => o,
            None => return (format!("No EPMA oxide of {element} in this project."), true),
        },
    };
    let factor = chemchron_core::constants::oxide_element_fraction(&oxide) * 1e4;
    let rules = app.settings.label_rules.clone();
    if !factor.is_finite() {
        return (format!("No molar mass for {oxide}."), true);
    }
    let mut by_label: BTreeMap<String, f64> = BTreeMap::new();
    for r in &app.data.rows {
        if truthy(r, "No_EPMA") {
            continue;
        }
        let wt = num(r, &format!("{oxide}_wt%")).or_else(|| num(r, &oxide));
        if let Some(wt) = wt.filter(|w| w.is_finite() && *w > 0.0) {
            let key = if rules.is_plain() { text(r, "Label").trim().to_string() } else { rules.epma_key(&text(r, "Label")) };
            by_label.insert(key, wt * factor);
        }
    }
    if by_label.is_empty() {
        return (format!("No EPMA analysis has {oxide} values."), true);
    }
    let mut n = 0;
    let mut missing = 0;
    let groups: Vec<(u64, bool)> = app.laser.session.groups.iter().map(|g| (g.id, g.kind == GroupKind::Sample)).collect();
    for s in app.laser.session.selections.iter_mut() {
        if !groups.iter().any(|(id, sample)| *id == s.group && *sample) {
            continue;
        }
        let key = if rules.is_plain() { spot_label(&s.label).trim().to_string() } else { rules.laser_key(&spot_label(&s.label)) };
        match by_label.get(&key) {
            Some(ppm) => {
                s.is_ppm = Some(*ppm);
                n += 1;
            }
            None => missing += 1,
        }
    }
    if n > 0 {
        app.laser.changed = true;
    }
    (format!("Internal standard from EPMA ({oxide} wt%): {n} laser spots set{}.", if missing > 0 { format!(", {missing} without an EPMA analysis of exactly the same label kept their value") } else { String::new() }), n == 0)
}
