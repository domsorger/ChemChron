// SPDX-License-Identifier: GPL-3.0-or-later
//! Reference-material recognition and comparison with accepted ages.

use crate::constants::DecimalPlaces;
use crate::row::*;
use serde_json::{Map, Value};

/// Reference materials by code: `{code: {"age", "age_2s", "mineral", ...}}`, in the user's order.
pub type Refs = Map<String, Value>;

pub const WITHIN_2S: &str = "within 2σ";
pub const OUTSIDE_2S: &str = "outside 2σ";

pub fn ref_num(refs: &Refs, code: &str, key: &str) -> Option<f64> {
    refs.get(code).and_then(|r| r.get(key)).and_then(|v| v.as_f64()).filter(|v| v.is_finite())
}

/// The codes that identify an entry in labels: its code, its alternative
/// codes (`"aliases"`) and its laser name and aliases.
pub fn codes_of(code: &str, entry: &Value) -> Vec<String> {
    let mut out = vec![code.to_string()];
    let mut add = |s: &str| {
        let s = s.trim();
        if !s.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(s)) {
            out.push(s.to_string());
        }
    };
    let list = |v: Option<&Value>| -> Vec<String> { v.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
    for a in list(entry.get("aliases")) {
        add(&a);
    }
    if let Some(l) = entry.get("laser") {
        if let Some(n) = l.get("name").and_then(|n| n.as_str()) {
            add(n);
        }
        for a in list(l.get("aliases")) {
            add(&a);
        }
    }
    out
}

/// Where `code` stands in `label` as a word: boundary before it, no digit after it.
fn code_in_label(lu: &[char], code: &str) -> bool {
    let cu: Vec<char> = code.to_uppercase().chars().collect();
    if cu.is_empty() || cu.len() > lu.len() {
        return false;
    }
    (0..=lu.len() - cu.len()).any(|idx| {
        lu[idx..idx + cu.len()] == cu[..] && (idx == 0 || !lu[idx - 1].is_alphanumeric()) && lu.get(idx + cu.len()).is_none_or(|c| !c.is_ascii_digit())
    })
}

/// A reference code (or one of its alternative codes) inside a label: word
/// boundary before it, no digit after it. Codes are tried first, then the
/// alternative codes.
pub fn guess_reference_from_label(label: &str, refs: &Refs) -> Option<String> {
    if label.is_empty() || refs.is_empty() {
        return None;
    }
    let lu: Vec<char> = label.to_uppercase().chars().collect();
    for code in refs.keys() {
        if code_in_label(&lu, code) {
            return Some(code.clone());
        }
    }
    for (code, e) in refs {
        if codes_of(code, e).iter().skip(1).any(|a| code_in_label(&lu, a)) {
            return Some(code.clone());
        }
    }
    None
}

pub fn nearest_reference_by_age(age_ma: f64, refs: &Refs) -> Option<String> {
    let mut best: Option<(String, f64)> = None;
    for code in refs.keys() {
        let Some(a) = ref_num(refs, code, "age") else { continue };
        let d = (age_ma - a).abs();
        if best.as_ref().is_none_or(|(_, bd)| d < *bd) {
            best = Some((code.clone(), d));
        }
    }
    best.map(|b| b.0)
}

pub fn normalize_within_values(rows: &mut [Row]) {
    for row in rows {
        for key in ["Ref_Within_2s", "Group_Within_2s"] {
            let v = text(row, key);
            if v == "within 2s" {
                set(row, key, jstr(WITHIN_2S));
            } else if v == "outside 2s" {
                set(row, key, jstr(OUTSIDE_2S));
            }
        }
    }
}

pub const REF_FIELDS: [&str; 5] = ["Ref_Accepted_Age_Ma", "Ref_Accepted_2s_Ma", "Ref_Diff_Ma", "Ref_Diff_pct", "Ref_Within_2s"];

pub fn compute_ref_fields(
    age_ma: Option<f64>,
    age_2s: Option<f64>,
    code: &str,
    refs: &Refs,
    is_reference: bool,
    dp: &DecimalPlaces,
) -> Vec<(&'static str, Value)> {
    let mut out: Vec<(&'static str, Value)> = REF_FIELDS.iter().map(|f| (*f, blank())).collect();
    if !is_reference || code.is_empty() || !refs.contains_key(code) {
        return out;
    }
    let Some(age) = age_ma else { return out };
    let Some(accepted) = ref_num(refs, code, "age") else { return out };
    let ref_2s = ref_num(refs, code, "age_2s").unwrap_or(0.0);
    let diff = age - accepted;
    let diff_pct = if accepted != 0.0 { 100.0 * diff / accepted } else { f64::NAN };
    let a2s = age_2s.unwrap_or(0.0);
    let combined = (a2s * a2s + ref_2s * ref_2s).sqrt();
    let within = diff.abs() <= combined;
    out[0].1 = jnum(accepted);
    out[1].1 = refs.get(code).and_then(|r| r.get("age_2s")).cloned().unwrap_or_else(blank);
    out[2].1 = jround(diff, dp.age);
    out[3].1 = jround(diff_pct, dp.age);
    out[4].1 = jstr(if within { WITHIN_2S } else { OUTSIDE_2S });
    out
}

pub fn recompute_ref_fields_for_row(row: &mut Row, refs: &Refs, dp: &DecimalPlaces) {
    let a2s = num(row, "Age_2s_Ma_total").or_else(|| num(row, "Age_2s_Ma"));
    let fields = compute_ref_fields(num(row, "Age_Ma"), a2s, &text(row, "Reference"), refs, truthy(row, "Is_Reference"), dp);
    for (k, v) in fields {
        set(row, k, v);
    }
}
