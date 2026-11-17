// SPDX-License-Identifier: GPL-3.0-or-later
//! The full result record of one EPMA analysis: age, uncertainty, mineral,
//! structural formula and reference-material comparison.

use crate::age::{propagate_age_error, solve_age_years, total_radiogenic_pb_ppm};
use crate::constants::*;
use crate::formula::{identify_mineral, write_mineral_fields, OxideWt};
use crate::reference::{compute_ref_fields, guess_reference_from_label, nearest_reference_by_age, Refs};
use crate::row::*;
use crate::uncertainty::{blank_uncertainty, uncertainty_fields};
use serde_json::Value;
use std::collections::BTreeMap;

/// A relative uncertainty that may differ per mineral.
#[derive(Clone, Debug, Default)]
pub enum PerMineral {
    #[default]
    None,
    All(f64),
    By(BTreeMap<String, f64>),
}

impl PerMineral {
    pub fn get(&self, mineral: &str) -> Option<f64> {
        match self {
            PerMineral::None => None,
            PerMineral::All(v) => Some(*v),
            PerMineral::By(m) => m.get(mineral).copied(),
        }
    }
}

pub struct Context<'a> {
    pub decay: &'a DecayConstants,
    pub refs: &'a Refs,
    pub active: Vec<String>,
    pub long_term: PerMineral,
    pub excess: PerMineral,
    pub dp: DecimalPlaces,
    pub thresholds: MineralIdThresholds,
}

pub fn new_row_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let c = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128;
    let mut x = t ^ (c.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835));
    // splitmix-style scramble to 128 bits of hex
    let mut out = String::with_capacity(32);
    for _ in 0..2 {
        let mut z = (x as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.push_str(&format!("{z:016x}"));
        x = (x >> 64) ^ (z as u128) ^ c;
    }
    out
}

pub fn ensure_row_ids(rows: &mut [Row]) -> usize {
    let mut n = 0;
    for r in rows.iter_mut() {
        if !truthy(r, "Row_ID") {
            set(r, "Row_ID", jstr(new_row_id()));
            n += 1;
        }
    }
    n
}

fn sigma_value(row: &Row, key: &str, fallback: f64) -> Result<f64, String> {
    if truthy(row, key) {
        locale_value(row.get(key), key)
    } else {
        Ok(fallback)
    }
}

pub fn process_row(row: &Row, ctx: &Context) -> Result<Row, String> {
    let dp = &ctx.dp;
    let c = ctx.decay;
    let label = text(row, "Label").trim().to_string();
    let th_wt = locale_value(row.get("ThO2"), "ThO2")?;
    let u_wt = locale_value(row.get("UO2"), "UO2")?;
    let pb_wt = locale_value(row.get("PbO"), "PbO")?;
    let fallback: Vec<&str> = PARENT_OXIDES.iter().copied().filter(|o| !truthy(row, &format!("sigma_{o}"))).collect();
    let s_th = sigma_value(row, "sigma_ThO2", DEFAULT_SIGMA_FALLBACK_REL_TH_U * th_wt)?;
    let s_u = sigma_value(row, "sigma_UO2", DEFAULT_SIGMA_FALLBACK_REL_TH_U * u_wt)?;
    let s_pb = sigma_value(row, "sigma_PbO", DEFAULT_SIGMA_FALLBACK_REL_PB * pb_wt)?;
    let (th, u, pb) = (th_wt * 1e4 * TH_FRAC, u_wt * 1e4 * U_FRAC, pb_wt * 1e4 * PB_FRAC);
    let (sth, su, spb) = (s_th * 1e4 * TH_FRAC, s_u * 1e4 * U_FRAC, s_pb * 1e4 * PB_FRAC);
    let t = solve_age_years(th, u, pb, c, 5.0e8);
    let age = t.map(|t| t / 1e6).unwrap_or(f64::NAN);
    let a2s = if t.is_some() { propagate_age_error(th, u, pb, sth, su, spb, c, t) / 1e6 } else { f64::NAN };
    let calc_pb_ppm = t.map(|t| total_radiogenic_pb_ppm(th, u, t, c)).unwrap_or(f64::NAN);
    let calc_pb_wt = calc_pb_ppm / 1e4 / PB_FRAC;
    let pb_diff = pb_wt - calc_pb_wt;

    let mut w: OxideWt = BTreeMap::new();
    w.insert("ThO2".into(), th_wt);
    w.insert("UO2".into(), u_wt);
    w.insert("PbO".into(), pb_wt);
    for ox in &ctx.active {
        w.insert(ox.clone(), locale_value(row.get(ox), ox)?);
    }
    let manual = text(row, "Mineral").trim().to_string();
    let mineral: String =
        if MINERAL_NAMES.contains(&manual.as_str()) { manual } else { identify_mineral(&w, &ctx.thresholds).to_string() };

    let age_r = if age.is_nan() { None } else { Some(age) };
    let a2s_r = if a2s.is_nan() { None } else { Some(a2s) };

    let manual_ref = text(row, "Reference").trim().to_string();
    let is_ref_input = row.get("Is_Reference");
    let guess = guess_reference_from_label(&label, ctx.refs);
    let explicit_false = matches!(is_ref_input, Some(Value::Bool(false)));
    let (code, is_ref) = if !manual_ref.is_empty() {
        (manual_ref, true)
    } else if guess.is_some() && !explicit_false {
        (guess.unwrap(), true)
    } else if matches!(is_ref_input, Some(Value::Bool(true)))
        || matches!(is_ref_input, Some(Value::String(s)) if s == "True" || s == "true" || s == "1")
        || matches!(is_ref_input, Some(Value::Number(n)) if n.as_f64() == Some(1.0))
    {
        (if age.is_nan() { None } else { nearest_reference_by_age(age, ctx.refs) }.unwrap_or_default(), true)
    } else {
        (String::new(), false)
    };

    let rel = if !a2s.is_nan() && !age.is_nan() && age != 0.0 { 100.0 * a2s / age } else { f64::NAN };
    let lt = ctx.long_term.get(&mineral);
    let ex = ctx.excess.get(&mineral);
    let unc = if (lt.is_none() && ex.is_none()) || age.is_nan() || a2s.is_nan() {
        blank_uncertainty()
    } else {
        uncertainty_fields(age_r, a2s_r, ex, lt, dp)
    };
    let total = match &unc[2].1 {
        Value::Number(n) => n.as_f64(),
        _ => a2s_r,
    };
    let ref_fields = compute_ref_fields(age_r, total, &code, ctx.refs, is_ref, dp);

    let mut out = Row::new();
    set(&mut out, "Label", jstr(&label));
    let rid = text(row, "Row_ID").trim().to_string();
    set(&mut out, "Row_ID", jstr(if rid.is_empty() { new_row_id() } else { rid }));
    set(&mut out, "Is_Reference", Value::Bool(is_ref));
    set(&mut out, "Reference", jstr(&code));
    set(&mut out, "Age_Ma", jopt(age_r));
    set(&mut out, "Age_2s_Ma", jopt(a2s_r));
    set(&mut out, "Age_Rel_2s_pct", jround(rel, dp.age));
    for (k, v) in unc {
        set(&mut out, k, v);
    }
    set(&mut out, "sigma_ThO2", jnum(s_th));
    set(&mut out, "sigma_UO2", jnum(s_u));
    set(&mut out, "sigma_PbO", jnum(s_pb));
    set(&mut out, "Sigma_Fallback", jstr(fallback.join(",")));
    set(&mut out, "Flag", jstr(text(row, "Flag").trim()));
    set(&mut out, "Include_In_Average", Value::Bool(parse_bool(row.get("Include_In_Average"), true)));
    set(&mut out, "Comment", jstr(text(row, "Comment").trim()));
    set(&mut out, "Calculated_PbO_wt%", jround(calc_pb_wt, dp.pb_calc));
    set(&mut out, "Pb_difference", jround(pb_diff, dp.pb_calc));
    // mineral-dependent fields (CSUTPS, ternary, exchange vector, sites, formula, apfu)
    let mut mf = Row::new();
    write_mineral_fields(&mut mf, &w, &ctx.active, &mineral, dp);
    for k in ["CSUTPS", "X_huttonite", "X_cheralite", "X_monazite", "X_LREE", "X_HREE", "ThUSi_af", "REEYP_af", "A_site_sum", "B_site_sum", "Mineral_Formula", "Mineral"] {
        if let Some(v) = mf.remove(k) {
            out.insert(k.to_string(), v);
        }
    }
    set(&mut out, "SampleGroup", jstr(text(row, "SampleGroup").trim()));
    set(&mut out, "EPMA_Session", jstr(text(row, "EPMA_Session").trim()));
    set(&mut out, "Below_DL", jstr(text(row, "Below_DL").trim()));
    for (k, v) in ref_fields {
        set(&mut out, k, v);
    }
    for (k, v) in crate::trace::trace_fields(row) {
        out.insert(k, v);
    }
    for (k, v) in crate::upb::upb_input_fields(row) {
        out.insert(k, v);
    }
    for (k, v) in crate::upb::compute_upb_fields(&out, c, dp.age) {
        out.insert(k, v);
    }
    let mut parents: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
    parents.extend(ctx.active.iter().cloned());
    for ox in &parents {
        set(&mut out, &format!("{ox}_wt%"), jnum(w.get(ox).copied().unwrap_or(0.0)));
        for extra in [format!("{ox}_2s_wt%"), format!("{ox}_DL_wt%")] {
            if let Some(v) = row.get(&extra) {
                if v != &blank() && !v.is_null() {
                    if let Some(x) = num_loose(row, &extra) {
                        set(&mut out, &extra, jnum(x));
                    }
                }
            }
        }
    }
    // apfu columns, in the original order (active then parents)
    for (k, v) in mf {
        out.insert(k, v);
    }
    Ok(out)
}

/// Raw-input-shaped row from a processed one, for reprocessing.
pub fn row_from_result(r: &Row, _active: &[String]) -> Row {
    let mut row = Row::new();
    for k in ["Label", "Row_ID", "SampleGroup", "EPMA_Session", "Flag", "Comment", "Reference", "Mineral", "Below_DL"] {
        set(&mut row, k, r.get(k).cloned().unwrap_or_else(blank));
    }
    set(&mut row, "Is_Reference", r.get("Is_Reference").cloned().unwrap_or(Value::Bool(false)));
    set(&mut row, "Include_In_Average", r.get("Include_In_Average").cloned().unwrap_or(Value::Bool(true)));
    let mut all = optional_oxides_master();
    all.extend(PARENT_OXIDES);
    for ox in all {
        let key = format!("{ox}_wt%");
        if let Some(v) = r.get(&key) {
            row.insert(ox.to_string(), v.clone());
        }
        for extra in [format!("{ox}_2s_wt%"), format!("{ox}_DL_wt%")] {
            if let Some(v) = r.get(&extra) {
                if v != &blank() && !v.is_null() {
                    row.insert(extra, v.clone());
                }
            }
        }
    }
    let fb: Vec<String> = text(r, "Sigma_Fallback").split(',').filter(|s| !s.is_empty()).map(String::from).collect();
    for (ox, legacy) in [("ThO2", 0.02), ("UO2", 0.02), ("PbO", 0.10)] {
        let sk = format!("sigma_{ox}");
        let Some(sv) = r.get(&sk) else { continue };
        if sv == &blank() || sv.is_null() || fb.iter().any(|f| f == ox) {
            continue;
        }
        if !r.contains_key("Sigma_Fallback") {
            if let (Some(s), Some(v)) = (sv.as_f64(), num(r, &format!("{ox}_wt%"))) {
                if v > 0.0 && (s - legacy * v).abs() <= 1e-9 * s.abs().max(1.0) {
                    continue;
                }
            }
        }
        row.insert(sk, sv.clone());
    }
    for (k, v) in crate::trace::trace_fields(r) {
        row.insert(k, v);
    }
    for (k, v) in crate::upb::upb_input_fields(r) {
        row.insert(k, v);
    }
    row
}
