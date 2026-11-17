// SPDX-License-Identifier: GPL-3.0-or-later
//! Isotopic U-(Th-)Pb ages from reduced LA-ICP-MS ratios stored on a row
//! ("UPb_" fields): ages, discordance, concordia distance, rejection criteria
//! and recognition of ratio columns in imported tables.

use crate::constants::DecayConstants;
use crate::row::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const U238_U235: f64 = 137.818;
pub const PREFIX: &str = "UPb_";
pub const RATIO_KEYS: [&str; 7] = ["r86", "r76", "rho_TW", "r68", "r75", "rho_W", "r82"];
pub const AGE_KEYS: [&str; 4] = ["t68", "t75", "t76", "t82"];
pub const CONC_DIST_FIELD: &str = "UPb_ConcDist_sigma";
pub const CONC_DIST_POINT_FIELD: &str = "UPb_ConcDist_point_sigma";
pub const INCLUDE_FIELD: &str = "UPb_Include";
pub const DEFAULT_DISC_THRESHOLD_PCT: f64 = 2.0;
pub const DEFAULT_SYSTEMATIC_PCT: f64 = 1.3;
pub const DEFAULT_DISC_HARD_LIMIT_PCT: f64 = 10.0;
pub const DEFAULT_MAX_REL_2SE_PCT: f64 = 10.0;
pub const RATIO_SIGNIFICANT_FIGURES: i32 = 5;

pub fn ellipse_scale_95() -> f64 {
    (-2.0 * 0.05f64.ln()).sqrt()
}

pub fn input_fields() -> Vec<String> {
    let mut v: Vec<String> = RATIO_KEYS.iter().map(|k| format!("{PREFIX}{k}")).collect();
    v.extend(["r86", "r76", "r68", "r75", "r82"].iter().map(|k| format!("{PREFIX}{k}_2s")));
    v.push(format!("{PREFIX}Source"));
    v.push(format!("{PREFIX}Session"));
    v
}

pub fn computed_fields() -> Vec<String> {
    let mut v: Vec<String> = AGE_KEYS.iter().map(|t| format!("{PREFIX}{t}_Ma")).collect();
    v.extend(AGE_KEYS.iter().map(|t| format!("{PREFIX}{t}_2s_Ma")));
    v.extend(["UPb_Disc76_pct", "UPb_Disc75_pct", "UPb_RatioConsistency_pct", CONC_DIST_FIELD].map(String::from));
    v
}

/// Ratio fields shown with 5 significant figures.
pub fn ratio_fields() -> Vec<String> {
    let mut v: Vec<String> = input_fields().into_iter().filter(|f| f != "UPb_Source" && f != "UPb_Session").collect();
    v.extend(["UPb_r86_2s_point", "UPb_r76_2s_point", "UPb_rho_TW_point", "UPb_r82_2s_point"].map(String::from));
    v
}

pub fn round_significant(v: f64, digits: i32) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let d = digits - 1 - v.abs().log10().floor() as i32;
    if d >= 0 {
        round_to(v, d as usize)
    } else {
        let p = 10f64.powi(-d);
        (v / p).round() * p
    }
}

/// `_num`: float() of numbers and numeric text, finite only.
pub fn n(row: &Row, key: &str) -> Option<f64> {
    match row.get(key) {
        Some(Value::Number(x)) => x.as_f64().filter(|v| v.is_finite()),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

pub fn has_upb(row: &Row) -> bool {
    ["r86", "r68", "r82"].iter().any(|k| n(row, &format!("{PREFIX}{k}")).is_some())
}

pub fn upb_input_fields(row: &Row) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for k in input_fields() {
        if let Some(v) = row.get(&k) {
            if !matches!(v, Value::Null) && v != &blank() {
                out.push((k, v.clone()));
            }
        }
    }
    if row.get(INCLUDE_FIELD) == Some(&Value::Bool(false)) {
        out.push((INCLUDE_FIELD.into(), Value::Bool(false)));
    }
    out
}

fn r76_model(t: f64, l8: f64, l5: f64) -> f64 {
    (l5 * t).exp_m1() / (U238_U235 * (l8 * t).exp_m1())
}

/// 207Pb/206Pb ratio of a concordant age (years).
pub fn r76_from_age(t: f64, c: &DecayConstants) -> Option<f64> {
    (t > 0.0).then(|| r76_model(t, c.lambda_238, c.lambda_235))
}

/// 207Pb/206Pb age in years.
pub fn age_76(r76: f64, c: &DecayConstants) -> Option<f64> {
    let (l8, l5) = (c.lambda_238, c.lambda_235);
    if r76 <= 0.0 {
        return None;
    }
    let (mut lo, mut hi) = (1.0e3, 6.0e9);
    let mut f_lo = r76_model(lo, l8, l5) - r76;
    let f_hi = r76_model(hi, l8, l5) - r76;
    if f_lo * f_hi > 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        let fm = r76_model(mid, l8, l5) - r76;
        if f_lo * fm <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
            f_lo = fm;
        }
    }
    Some(0.5 * (lo + hi))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TwPair {
    pub x: Option<f64>,
    pub sx: Option<f64>,
    pub y: Option<f64>,
    pub sy: Option<f64>,
    pub rho: Option<f64>,
}

/// Tera-Wasserburg pair, measured or derived from the Wetherill pair (2s).
pub fn tw_pair(row: &Row) -> TwPair {
    let mut p = TwPair {
        x: n(row, "UPb_r86"),
        sx: n(row, "UPb_r86_2s"),
        y: n(row, "UPb_r76"),
        sy: n(row, "UPb_r76_2s"),
        rho: n(row, "UPb_rho_TW"),
    };
    if p.x.is_some() && p.y.is_some() {
        return p;
    }
    let (r68, s68) = (n(row, "UPb_r68"), n(row, "UPb_r68_2s"));
    let (r75, s75) = (n(row, "UPb_r75"), n(row, "UPb_r75_2s"));
    let rho_w = n(row, "UPb_rho_W");
    let r68ok = r68.filter(|v| *v != 0.0 && *v > 0.0);
    if p.x.is_none() {
        if let Some(r68) = r68ok {
            let x = 1.0 / r68;
            p.x = Some(x);
            p.sx = s68.map(|s| x * s / r68);
        }
    }
    if p.y.is_none() {
        if let (Some(r68), Some(r75)) = (r68ok, r75.filter(|v| *v != 0.0)) {
            let y = r75 / (U238_U235 * r68);
            p.y = Some(y);
            if let (Some(s68), Some(s75)) = (s68, s75) {
                let (a, b, c) = (s75 / r75, s68 / r68, rho_w.unwrap_or(0.0));
                let ry = (a * a + b * b - 2.0 * c * a * b).max(0.0).sqrt();
                p.sy = Some(y * ry);
                p.rho = if ry > 0.0 && b > 0.0 { Some((b * b - c * a * b) / (b * ry)) } else { None };
            }
        }
    }
    p
}

fn nz(v: Option<f64>) -> Option<f64> {
    v.filter(|x| *x != 0.0)
}

/// Ages (Ma), 2s, discordance and checks for a row with U-Pb data.
pub fn compute_upb_fields(row: &Row, c: &DecayConstants, _age_dp: usize) -> Vec<(String, Value)> {
    if !has_upb(row) {
        return Vec::new();
    }
    let (l8, l5, l2) = (c.lambda_238, c.lambda_235, c.lambda_232);
    let mut out: BTreeMap<String, Value> = computed_fields().into_iter().map(|k| (k, blank())).collect();
    let p = tw_pair(row);
    let (mut t68, mut t75, mut t76) = (None, None, None);
    if let Some(x) = nz(p.x).filter(|x| *x > 0.0) {
        let t = (1.0 / x).ln_1p() / l8;
        t68 = Some(t);
        out.insert("UPb_t68_Ma".into(), jnum(t / 1e6));
        if let Some(sx) = p.sx {
            out.insert("UPb_t68_2s_Ma".into(), jnum(sx / (l8 * x * (x + 1.0)) / 1e6));
        }
    }
    if let Some(y) = nz(p.y).filter(|y| *y > 0.0) {
        if let Some(t) = age_76(y, c) {
            t76 = Some(t);
            out.insert("UPb_t76_Ma".into(), jnum(t / 1e6));
            if let Some(sy) = p.sy {
                let h = t * 1e-6;
                let d = (r76_model(t + h, l8, l5) - r76_model(t - h, l8, l5)) / (2.0 * h);
                if d != 0.0 {
                    out.insert("UPb_t76_2s_Ma".into(), jnum((sy / d).abs() / 1e6));
                }
            }
        }
    }
    if let (Some(x), Some(y)) = (nz(p.x).filter(|v| *v > 0.0), nz(p.y).filter(|v| *v > 0.0)) {
        let r75 = y * U238_U235 / x;
        let t = r75.ln_1p() / l5;
        t75 = Some(t);
        out.insert("UPb_t75_Ma".into(), jnum(t / 1e6));
        if let (Some(sx), Some(sy)) = (p.sx, p.sy) {
            let (a, b) = (sy / y, sx / x);
            let rel = (a * a + b * b - 2.0 * p.rho.unwrap_or(0.0) * a * b).max(0.0).sqrt();
            out.insert("UPb_t75_2s_Ma".into(), jnum(r75 * rel / (l5 * (1.0 + r75)) / 1e6));
        }
    }
    if let Some(r82) = n(row, "UPb_r82").filter(|v| *v > -1.0) {
        out.insert("UPb_t82_Ma".into(), jnum(r82.ln_1p() / l2 / 1e6));
        if let Some(s82) = n(row, "UPb_r82_2s") {
            out.insert("UPb_t82_2s_Ma".into(), jnum(s82 / (l2 * (1.0 + r82)) / 1e6));
        }
    }
    if let (Some(a), Some(b)) = (nz(t68), nz(t76)) {
        out.insert("UPb_Disc76_pct".into(), jnum(100.0 * (1.0 - a / b)));
    }
    if let (Some(a), Some(b)) = (nz(t68), nz(t75)) {
        out.insert("UPb_Disc75_pct".into(), jnum(100.0 * (1.0 - a / b)));
    }
    if let (Some(a), Some(b)) = (nz(n(row, "UPb_r68")), nz(n(row, "UPb_r86"))) {
        out.insert("UPb_RatioConsistency_pct".into(), jnum(100.0 * (a * b - 1.0)));
    }
    if let Some(d) = concordia_distance_sigma(row, c) {
        out.insert(CONC_DIST_FIELD.into(), jnum(d));
    }
    computed_fields().into_iter().map(|k| {
        let v = out.remove(&k).unwrap_or_else(blank);
        (k, v)
    }).collect()
}

pub fn tw_concordia(t: f64, l8: f64, l5: f64) -> (f64, f64) {
    let e8 = (l8 * t).exp_m1();
    (1.0 / e8, (l5 * t).exp_m1() / (U238_U235 * e8))
}

/// Smallest Mahalanobis distance (sigma) from the spot to the concordia (TW space).
pub fn concordia_distance_sigma(row: &Row, c: &DecayConstants) -> Option<f64> {
    let p = tw_pair(row);
    distance_for(p.x?, p.sx?, p.y?, p.sy?, p.rho, c)
}

pub fn distance_for(x: f64, sx: f64, y: f64, sy: f64, rho: Option<f64>, c: &DecayConstants) -> Option<f64> {
    if x <= 0.0 || y <= 0.0 || sx <= 0.0 || sy <= 0.0 {
        return None;
    }
    let (l8, l5) = (c.lambda_238, c.lambda_235);
    let (a, b) = (sx / 2.0, sy / 2.0);
    let r = rho.unwrap_or(0.0).clamp(-0.999, 0.999);
    let det = 1.0 - r * r;
    let d2 = |t: f64| {
        let (cx, cy) = tw_concordia(t, l8, l5);
        let (u, v) = ((cx - x) / a, (cy - y) / b);
        (u * u - 2.0 * r * u * v + v * v) / det
    };
    let t68 = (1.0 / x).ln_1p() / l8;
    let t76 = age_76(y, c).filter(|v| *v != 0.0).unwrap_or(t68);
    let lo = (0.5 * t68.min(t76)).max(1.0e5);
    let hi = 1.5 * t68.max(t76) + 1.0e7;
    let nn = 400;
    let ts: Vec<f64> = (0..=nn).map(|k| lo + (hi - lo) * k as f64 / nn as f64).collect();
    let vals: Vec<f64> = ts.iter().map(|t| d2(*t)).collect();
    let mut k = 0;
    for i in 1..vals.len() {
        if vals[i] < vals[k] {
            k = i;
        }
    }
    let (mut left, mut right) = (ts[k.saturating_sub(1)], ts[(k + 1).min(nn)]);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..60 {
        let m1 = right - g * (right - left);
        let m2 = left + g * (right - left);
        if d2(m1) < d2(m2) {
            right = m2;
        } else {
            left = m1;
        }
    }
    Some(d2(0.5 * (left + right)).min(vals[k]).max(0.0).sqrt())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Criteria {
    /// "76" or "75".
    pub definition: String,
    pub threshold_pct: f64,
    pub ellipse_test: bool,
    pub hard_limit_pct: Option<f64>,
    pub max_rel_2se_pct: Option<f64>,
}

impl Default for Criteria {
    fn default() -> Self {
        Criteria {
            definition: "76".into(),
            threshold_pct: DEFAULT_DISC_THRESHOLD_PCT,
            ellipse_test: true,
            hard_limit_pct: Some(DEFAULT_DISC_HARD_LIMIT_PCT),
            max_rel_2se_pct: Some(DEFAULT_MAX_REL_2SE_PCT),
        }
    }
}

pub fn is_discordant(row: &Row, c: &Criteria) -> Option<bool> {
    let v = n(row, &format!("UPb_Disc{}_pct", c.definition))?;
    if v.abs() <= c.threshold_pct {
        return Some(false);
    }
    if let Some(h) = c.hard_limit_pct {
        if v.abs() > h {
            return Some(true);
        }
    }
    if c.ellipse_test {
        let d = n(row, CONC_DIST_POINT_FIELD).or_else(|| n(row, CONC_DIST_FIELD));
        if d.is_some_and(|d| d <= ellipse_scale_95()) {
            return Some(false);
        }
    }
    Some(true)
}

pub fn relative_2se_pct(row: &Row) -> Vec<f64> {
    let p = tw_pair(row);
    let mut rels = Vec::new();
    for (v, s) in [(p.x, p.sx), (p.y, p.sy)] {
        if let (Some(v), Some(s)) = (v, s) {
            if v != 0.0 && v > 0.0 {
                rels.push(100.0 * s / v);
            }
        }
    }
    if rels.is_empty() {
        if let (Some(r), Some(s)) = (n(row, "UPb_r82"), n(row, "UPb_r82_2s")) {
            if r != 0.0 && r > 0.0 {
                rels.push(100.0 * s / r);
            }
        }
    }
    rels
}

pub fn is_imprecise(row: &Row, max: Option<f64>) -> bool {
    match max {
        None => false,
        Some(m) => relative_2se_pct(row).iter().any(|r| *r > m),
    }
}

/// Why a spot is left out of U-Pb means: "discordant", "imprecise" or None.
pub fn upb_rejection(row: &Row, c: &Criteria) -> Option<&'static str> {
    if is_discordant(row, c) == Some(true) {
        return Some("discordant");
    }
    if is_imprecise(row, c.max_rel_2se_pct) {
        return Some("imprecise");
    }
    None
}

pub fn included_in_upb(row: &Row) -> bool {
    row.get(INCLUDE_FIELD) != Some(&Value::Bool(false))
}

pub fn ensure_concordia_distance(rows: &mut [Row], c: &DecayConstants) {
    for row in rows.iter_mut() {
        if !row.contains_key(CONC_DIST_FIELD) && has_upb(row) {
            let d = concordia_distance_sigma(row, c);
            set(row, CONC_DIST_FIELD, d.map(jnum).unwrap_or_else(blank));
        }
    }
}

// ---- recognising ratio columns ------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnInfo {
    pub field: String,
    /// "value", "2s" or "1s".
    pub kind: String,
    pub prop: bool,
    pub relative: bool,
    pub index: usize,
    pub header: String,
}

fn masses_in(low: &str) -> Vec<u32> {
    let b: Vec<char> = low.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= b.len() {
        let is3 = b[i] == '2' && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit();
        let before = i > 0 && b[i - 1].is_ascii_digit();
        let after = i + 3 < b.len() && b[i + 3].is_ascii_digit();
        if is3 && !before && !after {
            out.push(b[i..i + 3].iter().collect::<String>().parse().unwrap());
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// Removes the first "<letters>ddd<letters> / <letters>ddd<letters>" ratio text.
fn strip_ratio(low: &str) -> String {
    let c: Vec<char> = low.chars().collect();
    let nn = c.len();
    // find a '/' with 3 digits on both sides (letters around allowed)
    for slash in 0..nn {
        if c[slash] != '/' {
            continue;
        }
        let mut l = slash;
        while l > 0 && c[l - 1] == ' ' {
            l -= 1;
        }
        let mut s = l;
        while s > 0 && c[s - 1].is_ascii_lowercase() {
            s -= 1;
        }
        if s < 3 || !(c[s - 3..s].iter().all(|d| d.is_ascii_digit())) {
            // digits directly before letters? pattern [a-z]*\d{3}[a-z]*
            let mut s2 = l;
            while s2 > 0 && c[s2 - 1].is_ascii_lowercase() {
                s2 -= 1;
            }
            let _ = s2;
        }
        // left part: [a-z]*\d{3}[a-z]* ending at l
        let mut j = l;
        while j > 0 && c[j - 1].is_ascii_lowercase() {
            j -= 1;
        }
        if j < 3 || !c[j - 3..j].iter().all(|d| d.is_ascii_digit()) {
            continue;
        }
        let mut start = j - 3;
        while start > 0 && c[start - 1].is_ascii_lowercase() {
            start -= 1;
        }
        // right part
        let mut r = slash + 1;
        while r < nn && c[r] == ' ' {
            r += 1;
        }
        let mut k = r;
        while k < nn && c[k].is_ascii_lowercase() {
            k += 1;
        }
        if k + 3 > nn || !c[k..k + 3].iter().all(|d| d.is_ascii_digit()) {
            continue;
        }
        let mut end = k + 3;
        while end < nn && c[end].is_ascii_lowercase() {
            end += 1;
        }
        let mut s: String = c[..start].iter().collect();
        s.push(' ');
        s.extend(c[end..].iter());
        return s;
    }
    low.to_string()
}

fn has_word(rest: &str, w: &str) -> bool {
    let c: Vec<char> = rest.chars().collect();
    let wc: Vec<char> = w.chars().collect();
    let isw = |ch: char| ch.is_alphanumeric() || ch == '_';
    for i in 0..c.len() {
        if i + wc.len() <= c.len() && c[i..i + wc.len()] == wc[..] {
            let b = i == 0 || !isw(c[i - 1]);
            let a = i + wc.len() == c.len() || !isw(c[i + wc.len()]);
            if a && b {
                return true;
            }
        }
    }
    false
}

fn has_n_s(rest: &str, d: char) -> bool {
    let c: Vec<char> = rest.chars().collect();
    for i in 0..c.len() {
        if c[i] == d {
            let mut j = i + 1;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if j < c.len() && (c[j] == 's' || c[j] == 'σ') {
                return true;
            }
        }
    }
    false
}

pub fn parse_upb_header(header: &str) -> Option<(String, String, bool, bool)> {
    let text = header.trim();
    let low = text.to_lowercase();
    if low.is_empty() || low.contains("age") || low.starts_with("t.") || low.contains("err[") {
        return None;
    }
    let m = masses_in(&low);
    if low.contains("rho") {
        if m.len() >= 4 && m[..4] == [206, 238, 207, 235] {
            return Some(("rho_W".into(), "value".into(), false, false));
        }
        if m.len() >= 4 && (m[..4] == [207, 206, 238, 206] || m[..4] == [238, 206, 207, 206]) {
            return Some(("rho_TW".into(), "value".into(), false, false));
        }
        return None;
    }
    if m.len() < 2 || !low.contains('/') {
        return None;
    }
    let field = match (m[0], m[1]) {
        (206, 238) => "r68",
        (207, 235) => "r75",
        (207, 206) => "r76",
        (238, 206) => "r86",
        (208, 232) => "r82",
        _ => return None,
    };
    let mut rest = strip_ratio(&low);
    if let Some(r) = rest.trim_start().strip_prefix("final") {
        if r.starts_with(char::is_whitespace) {
            rest = format!(" {}", r.trim_start());
        }
    }
    let kind = if has_n_s(&rest, '2') {
        "2s"
    } else if has_n_s(&rest, '1')
        || has_word(&rest, "se")
        || has_word(&rest, "sd")
        || rest.contains("_se")
        || rest.contains("_sd")
        || rest.contains('σ')
        || rest.contains("err")
    {
        "1s"
    } else {
        "value"
    };
    let relative = kind != "value" && (rest.contains('%') || rest.contains("rel"));
    Some((field.into(), kind.into(), rest.contains("prop"), relative))
}

pub fn plan_upb_columns(header: &[String], label_col: usize) -> (Vec<ColumnInfo>, Vec<String>) {
    let mut cols = Vec::new();
    let mut ignored = Vec::new();
    for (i, h) in header.iter().enumerate() {
        if i == label_col {
            continue;
        }
        match parse_upb_header(h) {
            None => {
                if !h.trim().is_empty() {
                    ignored.push(h.clone());
                }
            }
            Some((field, kind, prop, relative)) => cols.push(ColumnInfo { field, kind, prop, relative, index: i, header: h.clone() }),
        }
    }
    let prop: Vec<(String, String)> = cols.iter().filter(|c| c.prop).map(|c| (c.field.clone(), c.kind.clone())).collect();
    let mut keep: Vec<ColumnInfo> = Vec::new();
    for c in cols {
        if c.kind != "value" && !c.prop && prop.contains(&(c.field.clone(), c.kind.clone())) {
            ignored.push(format!("{} (2SE(prop) used instead)", c.header));
            continue;
        }
        if keep.iter().any(|k| k.field == c.field && k.kind == c.kind) {
            ignored.push(format!("{} (duplicate)", c.header));
            continue;
        }
        keep.push(c);
    }
    (keep, ignored)
}

pub fn guess_group_column(header: &[String], label_col: usize) -> Option<usize> {
    for wanted in ["sample group", "sample_group", "samplegroup", "group", "flag", "sample"] {
        for (i, h) in header.iter().enumerate() {
            if i != label_col && h.trim().to_lowercase() == wanted {
                return Some(i);
            }
        }
    }
    None
}

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub label: String,
    pub key: String,
    pub values: Row,
    pub group: String,
}

pub fn build_upb_records(table: &[Vec<String>], header_row: usize, label_col: usize, columns: &[ColumnInfo], group_col: Option<usize>) -> (Vec<Record>, Vec<String>) {
    let mut records = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut dups = Vec::new();
    for row in table.iter().skip(header_row + 1) {
        let Some(label) = row.get(label_col).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else { continue };
        let mut values: BTreeMap<String, BTreeMap<String, (f64, bool)>> = BTreeMap::new();
        for c in columns {
            let raw = row.get(c.index).map(|s| s.as_str()).unwrap_or("");
            let (Some(v), _) = crate::trace::parse_cell(raw) else { continue };
            values.entry(c.field.clone()).or_default().insert(c.kind.clone(), (v, c.relative));
        }
        let mut fields = Row::new();
        for (field, kinds) in &values {
            let Some((value, _)) = kinds.get("value") else { continue };
            set(&mut fields, &format!("{PREFIX}{field}"), jnum(*value));
            for (kind, factor) in [("2s", 1.0), ("1s", 2.0)] {
                if let Some((err, rel)) = kinds.get(kind) {
                    if field.starts_with('r') {
                        let e = err * factor;
                        let v = if *rel { value.abs() * e / 100.0 } else { e };
                        set(&mut fields, &format!("{PREFIX}{field}_2s"), jnum(v));
                        break;
                    }
                }
            }
        }
        if !["UPb_r86", "UPb_r68", "UPb_r82"].iter().any(|k| fields.contains_key(*k)) {
            continue;
        }
        let key = crate::trace::normalize_label(&label);
        if seen.contains(&key) {
            dups.push(label);
            continue;
        }
        seen.push(key.clone());
        let group = group_col.and_then(|g| row.get(g)).map(|s| s.trim().to_string()).unwrap_or_default();
        records.push(Record { label, key, values: fields, group });
    }
    (records, dups)
}

/// Field updates attaching an imported record to a row.
pub fn upb_update_for_row(row: &Row, rec: &Record, source: &str, session: Option<&str>) -> Row {
    let mut up = Row::new();
    for k in input_fields() {
        if k.starts_with(PREFIX) && k != "UPb_Source" && row.contains_key(&k) {
            set(&mut up, &k, blank());
        }
    }
    // marks of laser results from the LA-ICP-MS part (an import from a file has none)
    for k in ["UPb_Laser_Excess", "UPb_Laser_Excess_r86", "UPb_Laser_Excess_r76", "UPb_Laser_Excess_r82", "UPb_Laser_Kind", "UPb_Laser_RM", "UPb_t68_PrimaryRM_2s_pct", "UPb_t75_PrimaryRM_2s_pct", "UPb_t76_PrimaryRM_2s_pct", "UPb_t82_PrimaryRM_2s_pct"] {
        if row.contains_key(k) {
            set(&mut up, k, blank());
        }
    }
    for (k, v) in &rec.values {
        up.insert(k.clone(), v.clone());
    }
    let sess = session.map(String::from).unwrap_or_else(|| crate::sessions::session_name_from_path(source));
    set(&mut up, "UPb_Session", jstr(sess));
    let mut sources: Vec<String> = text(row, "UPb_Source").split("; ").filter(|s| !s.is_empty()).map(String::from).collect();
    if !sources.iter().any(|s| s == source) {
        sources.push(source.to_string());
    }
    set(&mut up, "UPb_Source", jstr(sources.join("; ")));
    up
}

pub fn reference_for_label(label: &str, prefixes: &[(String, String)]) -> Option<String> {
    let low = label.trim().to_lowercase();
    let mut p: Vec<&(String, String)> = prefixes.iter().collect();
    p.sort_by_key(|x| std::cmp::Reverse(x.0.len()));
    p.into_iter().find(|(pre, _)| !pre.is_empty() && low.starts_with(&pre.to_lowercase())).map(|x| x.1.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers() {
        assert_eq!(parse_upb_header("Final U238/Pb206_mean").unwrap().0, "r86");
        let h = parse_upb_header("Final U238/Pb206_2SE(prop)").unwrap();
        assert_eq!((h.1.as_str(), h.2), ("2s", true));
        assert_eq!(parse_upb_header("2s (%) 238U/206Pb").unwrap().1, "2s");
        assert!(parse_upb_header("2s (%) 238U/206Pb").unwrap().3);
        assert_eq!(parse_upb_header("rho 206Pb/238U v 207Pb/235U").unwrap().0, "rho_W");
        assert!(parse_upb_header("Final Age206_238").is_none());
        assert_eq!(parse_upb_header("207Pb/206Pb 1SE").unwrap().1, "1s");
    }
    #[test]
    fn ages() {
        let c = DecayConstants::default();
        let mut r = Row::new();
        // 500 Ma concordant
        let t = 500e6;
        let r68 = (c.lambda_238 * t).exp_m1();
        let r75 = (c.lambda_235 * t).exp_m1();
        set(&mut r, "UPb_r68", jnum(r68));
        set(&mut r, "UPb_r75", jnum(r75));
        let f: BTreeMap<String, Value> = compute_upb_fields(&r, &c, 2).into_iter().collect();
        assert!((f["UPb_t68_Ma"].as_f64().unwrap() - 500.0).abs() < 0.01);
        assert!((f["UPb_t75_Ma"].as_f64().unwrap() - 500.0).abs() < 0.2);
    }
}
