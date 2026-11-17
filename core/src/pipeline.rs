// SPDX-License-Identifier: GPL-3.0-or-later
//! The steps ChemChron runs after every change: (re)process rows, resolve
//! and apply both uncertainty models, group checks and U-Pb reference checks.

use crate::constants::PARENT_OXIDES;
use crate::epma::{process_csv_file, Imported};
use crate::overview::{annotate_upb_reference_checks, UpbOptions};
use crate::processing::{process_row, row_from_result, Context, PerMineral};
use crate::row::*;
use crate::settings::Settings;
use crate::uncertainty::{apply_uncertainty_model, resolve_uncertainty_model, Model};
use crate::upbstats::{apply_upb_uncertainty_model, resolve_upb_systematic, resolve_upb_uncertainty, UpbMineralModel};
use std::collections::BTreeMap;
use std::path::Path;

pub struct Models {
    pub chem: Model,
    pub upb: BTreeMap<String, UpbMineralModel>,
}

pub fn chem_model(rows: &[Row], s: &Settings) -> Model {
    let mut m = resolve_uncertainty_model(rows, &s.long_term_log, &s.uncertainty);
    if s.upb.decay_in_systematic {
        // chemical ages: λ²³⁸U as the decay-constant term (an upper bound for
        // Th-rich minerals; λ²³²Th has no stated uncertainty)
        let p = chemical_decay_rel_2s_pct(&s.decay);
        for v in m.values_mut() {
            v.systematic_pct = (v.systematic_pct.powi(2) + p * p).sqrt();
            v.systematic_source.push_str(&format!(" + decay constants {p:.2} %"));
        }
    }
    m
}

/// Relative 2σ (%) of a chemical age from the decay constants (λ²³⁸U).
pub fn chemical_decay_rel_2s_pct(c: &crate::constants::DecayConstants) -> f64 {
    2.0 * c.lambda_238_rel_1s_pct
}

pub fn context<'a>(s: &'a Settings, refs: &'a crate::reference::Refs, active: &[String], model: &Model) -> Context<'a> {
    Context {
        decay: &s.decay,
        refs,
        active: active.to_vec(),
        long_term: PerMineral::By(model.iter().map(|(k, v)| (k.clone(), v.systematic_pct)).collect()),
        excess: PerMineral::By(model.iter().map(|(k, v)| (k.clone(), v.excess_pct)).collect()),
        dp: s.decimals,
        thresholds: s.thresholds.clone(),
    }
}

/// Recomputes every row from its stored inputs (after a settings change).
pub fn reprocess_all(rows: &[Row], active: &[String], s: &Settings) -> Vec<Row> {
    let refs = s.refs();
    let model = chem_model(rows, s);
    let ctx = context(s, &refs, active, &model);
    rows.iter()
        .map(|r| {
            if truthy(r, "No_EPMA") {
                let mut r = r.clone();
                for (k, v) in crate::upb::compute_upb_fields(&r, &s.decay, s.decimals.age) {
                    r.insert(k, v);
                }
                r
            } else {
                let mut out = process_row(&row_from_result(r, active), &ctx).unwrap_or_else(|_| r.clone());
                // keep project-level fields process_row doesn't produce
                for k in ["UPb_Session", "UPb_PointKey"] {
                    if let Some(v) = r.get(k) {
                        out.entry(k.to_string()).or_insert_with(|| v.clone());
                    }
                }
                out
            }
        })
        .collect()
}

pub fn upb_options(s: &Settings, all_age_types: bool) -> UpbOptions {
    UpbOptions {
        age_type: s.upb.summary_age_type.clone(),
        systematic_pct: s.upb.systematic_pct,
        systematic: {
            let mut r = resolve_upb_systematic(&s.upb.by_mineral(), &s.upb_log);
            r.decay = s.upb.decay_in_systematic.then(|| s.decay.clone());
            r
        },
        criteria: s.upb.criteria(),
        all_age_types,
    }
}

/// Everything derived from the rows that must follow any edit.
pub fn refresh(rows: &mut [Row], s: &Settings) -> Models {
    let refs = s.refs();
    let chem = chem_model(rows, s);
    apply_uncertainty_model(rows, &chem, &refs, &s.decimals);
    let crit = s.upb.criteria();
    let upb = resolve_upb_uncertainty(rows, &s.upb.by_mineral(), &crit);
    apply_upb_uncertainty_model(rows, &upb, &s.decay, s.decimals.age);
    crate::stats::annotate_group_outliers(rows, &s.decimals);
    crate::upb::ensure_concordia_distance(rows, &s.decay);
    annotate_upb_reference_checks(rows, &refs, &upb_options(s, false), s.decimals.age, &s.decay);
    Models { chem, upb }
}

/// Imports EPMA files (each its own session) with the current model.
pub fn import_epma(paths: &[(std::path::PathBuf, String)], existing: &[Row], s: &Settings) -> (Vec<Row>, Vec<String>, Vec<String>) {
    let refs = s.refs();
    let model = chem_model(existing, s);
    let mut rows = Vec::new();
    let mut active: Vec<String> = Vec::new();
    let mut messages = Vec::new();
    for (p, session) in paths {
        let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let r: Result<Imported, String> = process_csv_file(p, &|act| context(s, &refs, act, &model));
        match r {
            Ok(imp) => {
                for mut row in imp.results {
                    set(&mut row, "EPMA_Session", jstr(session));
                    rows.push(row);
                }
                for o in imp.active {
                    if !active.contains(&o) {
                        active.push(o);
                    }
                }
                messages.extend(imp.skipped.into_iter().map(|m| format!("{name} — {m}")));
            }
            Err(e) => messages.push(e),
        }
    }
    (rows, active, messages)
}

pub fn order_active(active: &[String]) -> Vec<String> {
    crate::constants::optional_oxides_master().into_iter().filter(|o| active.iter().any(|a| a == o)).map(String::from).collect()
}

pub fn parents_and(active: &[String]) -> Vec<String> {
    let mut v: Vec<String> = PARENT_OXIDES.iter().map(|x| x.to_string()).collect();
    v.extend(active.iter().cloned());
    v
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()
}
