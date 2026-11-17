// SPDX-License-Identifier: GPL-3.0-or-later
//! Summary tables: per reference material and per sample group (Flag +
//! mineral), with chemical and, when present, isotopic statistics.

use crate::constants::DecimalPlaces;
use crate::reference::{ref_num, Refs};
use crate::row::*;
use crate::stats::{mineral_of, usable_for_age_statistics, weighted_mean_for_rows};
use crate::upb::{has_upb, Criteria};
use crate::upbstats::{isotopic_group_stats, systematic_for, upb_overview_reference_fields, upb_reference_fields, UpbSystematic, AGE_TYPES, UPB_REF_FIELDS};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct UpbOptions {
    pub age_type: String,
    pub systematic_pct: f64,
    pub systematic: UpbSystematic,
    pub criteria: Criteria,
    pub all_age_types: bool,
}

fn upb_columns(rows: &[&Row], o: &UpbOptions, dp: usize, reject: bool, k: f64, age_type: Option<&str>) -> Vec<(String, Value)> {
    let t = age_type.unwrap_or(&o.age_type);
    let (sys, _) = systematic_for(&o.systematic, rows, t, o.systematic_pct);
    isotopic_group_stats(rows, t, dp, sys, reject, k, &o.criteria)
}

fn upb_all_types(rows: &[&Row], o: &UpbOptions, dp: usize, reject: bool, k: f64, refs: Option<(&Refs, &str)>) -> Vec<(String, Value)> {
    if !o.all_age_types {
        return Vec::new();
    }
    let mut out = Vec::new();
    for t in AGE_TYPES {
        let mut cols = upb_columns(rows, o, dp, reject, k, Some(t));
        if cols.is_empty() {
            return Vec::new();
        }
        if let Some((r, code)) = refs {
            let wm = cols.iter().find(|c| c.0 == "UPb_WM_Ma").and_then(|c| c.1.as_f64());
            cols.extend(upb_overview_reference_fields(wm, r, code, t, dp));
        }
        out.extend(cols.into_iter().map(|(k, v)| (format!("UPbSum_{t}_{}", &k[4..]), v)));
    }
    out
}

fn put(r: &mut Row, kv: Vec<(String, Value)>) {
    for (k, v) in kv {
        r.insert(k, v);
    }
}

pub fn reference_overview(results: &[Row], refs: &Refs, dp: &DecimalPlaces, reject: bool, k: f64, upb: Option<&UpbOptions>) -> Vec<Row> {
    let mut groups: Vec<(String, Vec<&Row>)> = Vec::new();
    for r in results {
        if !truthy(r, "Is_Reference") {
            continue;
        }
        let code = text(r, "Reference");
        if code.is_empty() {
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == code) {
            Some(g) => g.1.push(r),
            None => groups.push((code, vec![r])),
        }
    }
    let mut out = Vec::new();
    for (code, all) in groups {
        let mut up = Vec::new();
        if let Some(o) = upb {
            up = upb_columns(&all, o, dp.age, reject, k, None);
            if !up.is_empty() {
                let wm = up.iter().find(|c| c.0 == "UPb_WM_Ma").and_then(|c| c.1.as_f64());
                up.extend(upb_overview_reference_fields(wm, refs, &code, &o.age_type, dp.age));
                up.extend(upb_all_types(&all, o, dp.age, reject, k, Some((refs, &code))));
            }
        }
        let rows: Vec<&Row> = all.iter().copied().filter(|r| usable_for_age_statistics(r)).collect();
        let ages: Vec<f64> = rows.iter().filter_map(|r| num(r, "Age_Ma")).collect();
        let mut o = Row::new();
        set(&mut o, "Reference", jstr(&code));
        if ages.is_empty() {
            if !up.is_empty() {
                let m = all.iter().map(|r| text(r, "Mineral")).find(|m| !m.is_empty()).unwrap_or_else(|| "Monazite".into());
                set(&mut o, "Mineral", jstr(m));
                set(&mut o, "N", jnum(0.0));
                set(&mut o, "Accepted_Age_Ma", jopt(ref_num(refs, &code, "age")));
                set(&mut o, "Accepted_2s_Ma", jopt(ref_num(refs, &code, "age_2s")));
                put(&mut o, up);
                out.push(o);
            }
            continue;
        }
        let m = mean(&ages);
        let sd2 = if ages.len() > 1 { 2.0 * pstdev(&ages) } else { 0.0 };
        let acc = ref_num(refs, &code, "age");
        let w = weighted_mean_for_rows(&rows, reject, k);
        let compared = w.as_ref().map(|w| w.mean).unwrap_or(m);
        let diff = acc.map(|a| compared - a);
        let diff_pct = match (diff, acc) {
            (Some(d), Some(a)) if a != 0.0 => Some(100.0 * d / a),
            _ => None,
        };
        let mineral = rows.iter().map(|r| text(r, "Mineral")).find(|m| !m.is_empty()).unwrap_or_else(|| "Monazite".into());
        set(&mut o, "Mineral", jstr(mineral));
        set(&mut o, "N", jnum(ages.len() as f64));
        set(&mut o, "Mean_Age_Ma", jround(m, dp.age));
        set(&mut o, "2s_Ma", jround(sd2, dp.age));
        set(&mut o, "Accepted_Age_Ma", if acc.is_some() { jopt(acc) } else { blank() });
        set(&mut o, "Accepted_2s_Ma", if acc.is_some() { jopt(ref_num(refs, &code, "age_2s")) } else { blank() });
        set(&mut o, "Diff_Ma", diff.map(|d| jround(d, dp.age)).unwrap_or_else(blank));
        set(&mut o, "Diff_pct", diff_pct.map(|d| jround(d, dp.age)).unwrap_or_else(blank));
        set(&mut o, "WeightedMean_Age_Ma", w.as_ref().map(|w| jround(w.mean, dp.age)).unwrap_or_else(blank));
        set(&mut o, "WeightedMean_2s_Ma", w.as_ref().map(|w| jround(w.total_2s, dp.age)).unwrap_or_else(blank));
        set(&mut o, "MSWD", w.as_ref().map(|w| jround(w.mswd, 2)).unwrap_or_else(blank));
        put(&mut o, up);
        out.push(o);
    }
    out
}

pub fn flag_overview(results: &[Row], dp: &DecimalPlaces, reject: bool, k: f64, upb: Option<&UpbOptions>) -> Vec<Row> {
    let mut groups: Vec<((String, String), Vec<&Row>)> = Vec::new();
    for r in results {
        let flag = text(r, "Flag");
        if flag.is_empty() {
            continue;
        }
        let key = (flag, mineral_of(r));
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(r),
            None => groups.push((key, vec![r])),
        }
    }
    let mut out = Vec::new();
    for ((flag, mineral), all) in groups {
        let mut up = Vec::new();
        if let Some(o) = upb {
            up = upb_columns(&all, o, dp.age, reject, k, None);
            if !up.is_empty() {
                up.extend(upb_all_types(&all, o, dp.age, reject, k, None));
            }
        }
        let rows: Vec<&Row> = all.iter().copied().filter(|r| usable_for_age_statistics(r)).collect();
        let ages: Vec<f64> = rows.iter().filter_map(|r| num(r, "Age_Ma")).collect();
        let mut o = Row::new();
        set(&mut o, "Flag", jstr(&flag));
        set(&mut o, "Mineral", jstr(&mineral));
        if ages.is_empty() {
            if !up.is_empty() {
                set(&mut o, "N", jnum(0.0));
                put(&mut o, up);
                out.push(o);
            }
            continue;
        }
        let w = weighted_mean_for_rows(&rows, reject, k);
        set(&mut o, "N", jnum(ages.len() as f64));
        set(&mut o, "Mean_Age_Ma", jround(mean(&ages), dp.age));
        set(&mut o, "2s_Ma", jround(if ages.len() > 1 { 2.0 * pstdev(&ages) } else { 0.0 }, dp.age));
        set(&mut o, "WeightedMean_Age_Ma", w.as_ref().map(|w| jround(w.mean, dp.age)).unwrap_or_else(blank));
        set(&mut o, "WeightedMean_2s_Ma", w.as_ref().map(|w| jround(w.total_2s, dp.age)).unwrap_or_else(blank));
        set(&mut o, "MSWD", w.as_ref().map(|w| jround(w.mswd, 2)).unwrap_or_else(blank));
        put(&mut o, up);
        out.push(o);
    }
    out
}

/// Per-row comparison of reference spots with their accepted U-Pb age.
pub fn annotate_upb_reference_checks(rows: &mut [Row], refs: &Refs, o: &UpbOptions, dp: usize, decay: &crate::constants::DecayConstants) {
    for i in 0..rows.len() {
        if truthy(&rows[i], "Ref_Within_2s") && num(&rows[i], "Age_Ma").is_none() {
            for k in crate::reference::REF_FIELDS {
                set(&mut rows[i], k, blank());
            }
        }
        // total 2σ of every spot: analytical + excess and the systematic term in quadrature
        for t in AGE_TYPES {
            let key = format!("UPb_{t}_2s_total_Ma");
            // from the full-precision age and 2σ (not the rounded ones shown)
            let full = crate::upbstats::full_age_point(&rows[i], t, decay).or_else(|| crate::upbstats::age_and_sigma(&rows[i], t).map(|(a, s1)| (a, 2.0 * s1)));
            let v = match full {
                Some((age, s2)) if age.is_finite() && s2.is_finite() => {
                    let (p, _) = systematic_for(&o.systematic, &[&rows[i]], t, o.systematic_pct);
                    jnum(s2.hypot(p / 100.0 * age))
                }
                _ => blank(),
            };
            if v != blank() || rows[i].contains_key(&key) {
                set(&mut rows[i], &key, v);
            }
        }
        let (sys, _) = systematic_for(&o.systematic, &[&rows[i]], &o.age_type, o.systematic_pct);
        let fields = upb_reference_fields(&rows[i], refs, &o.age_type, sys, dp);
        let any_new = fields.iter().any(|(_, v)| v != &blank());
        let any_old = UPB_REF_FIELDS.iter().any(|k| rows[i].get(*k).is_some_and(|v| v != &blank() && !v.is_null()));
        if any_new || any_old {
            for (k, v) in fields {
                set(&mut rows[i], k, v);
            }
        }
    }
}

pub fn any_upb(rows: &[Row]) -> bool {
    rows.iter().any(has_upb)
}
