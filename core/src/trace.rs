// SPDX-License-Identifier: GPL-3.0-or-later
//! Externally measured element concentrations (e.g. LA-ICP-MS trace
//! elements) attached to analyses by label, in ppm. Never used for ages.

use crate::constants::{element_symbol, oxide_element_fraction, oxide_molar_mass};
use crate::row::*;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const ELEMENT_SYMBOLS: [&str; 92] = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl", "Ar", "K", "Ca", "Sc",
    "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As", "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr", "Nb",
    "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In", "Sn", "Sb", "Te", "I", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd",
    "Pm", "Sm", "Eu", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf", "Ta", "W", "Re", "Os", "Ir", "Pt", "Au",
    "Hg", "Tl", "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "U",
];
const ORDER_HEAD: [&str; 19] =
    ["La", "Ce", "Pr", "Nd", "Sm", "Eu", "Gd", "Tb", "Dy", "Y", "Ho", "Er", "Tm", "Yb", "Lu", "Sc", "Th", "U", "Pb"];
pub const TE_BELOW_DL: &str = "TE_Below_DL";
pub const TE_SOURCE: &str = "TE_Source";

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

pub fn is_element(s: &str) -> bool {
    ELEMENT_SYMBOLS.contains(&s)
}

pub fn canonical_order<I: IntoIterator<Item = String>>(els: I) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for e in els {
        if !v.contains(&e) {
            v.push(e);
        }
    }
    let mut head: Vec<String> = ORDER_HEAD.iter().filter(|e| v.iter().any(|x| x == *e)).map(|e| e.to_string()).collect();
    let mut rest: Vec<String> = v.into_iter().filter(|e| !ORDER_HEAD.contains(&e.as_str())).collect();
    rest.sort_by_key(|e| ELEMENT_SYMBOLS.iter().position(|s| s == e).unwrap_or(999));
    head.extend(rest);
    head
}

/// `<El>_ppm`, `<El>_2s_ppm` or `<El>_LOD_ppm`: (element, kind).
pub fn trace_field(key: &str) -> Option<(&str, &str)> {
    let (el, kind) = key.split_once('_')?;
    let ok_el = el.len() <= 2 && el.chars().next()?.is_ascii_uppercase() && el.chars().skip(1).all(|c| c.is_ascii_lowercase());
    if ok_el && matches!(kind, "ppm" | "2s_ppm" | "LOD_ppm") {
        Some((el, kind))
    } else {
        None
    }
}

pub fn trace_elements_in_rows(rows: &[Row]) -> Vec<String> {
    let mut found = Vec::new();
    for row in rows {
        for k in row.keys() {
            if let Some((el, "ppm")) = trace_field(k) {
                if is_element(el) && !found.iter().any(|f: &String| f == el) {
                    found.push(el.to_string());
                }
            }
        }
    }
    canonical_order(found)
}

pub fn trace_columns(els: &[String]) -> Vec<String> {
    els.iter().flat_map(|e| [format!("{e}_ppm"), format!("{e}_2s_ppm"), format!("{e}_LOD_ppm")]).collect()
}

pub fn trace_fields(row: &Row) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> =
        row.iter().filter(|(k, _)| trace_field(k).is_some()).map(|(k, v)| (k.clone(), v.clone())).collect();
    for k in [TE_BELOW_DL, TE_SOURCE] {
        if truthy(row, k) {
            out.push((k.to_string(), row[k].clone()));
        }
    }
    out
}

pub fn te_below_dl(row: &Row) -> Vec<String> {
    text(row, TE_BELOW_DL).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Determined imported value (ppm), or None (missing, <= 0, below detection limit).
pub fn trace_value(row: &Row, el: &str) -> Option<f64> {
    if te_below_dl(row).iter().any(|e| e == el) {
        return None;
    }
    num(row, &format!("{el}_ppm")).filter(|v| *v > 0.0)
}

/// Case-insensitive, trimmed, leading zeros of the final number ignored.
pub fn normalize_label(label: &str) -> String {
    let t = label.trim().to_lowercase();
    let digits: String = t.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    if digits.is_empty() {
        return t;
    }
    let head = &t[..t.len() - digits.len()];
    let trimmed = digits.trim_start_matches('0');
    format!("{head}{}", if trimmed.is_empty() { "0" } else { trimmed })
}

/// Grain of an analysis: its label without a final "_<n>" or "-<n>".
pub fn grain_key(label: &str) -> String {
    let n = normalize_label(label);
    let r = re(&GRAIN_RE, r"[_\-]\d+$");
    r.replace(&n, "").to_string()
}
static GRAIN_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub struct TraceColumn {
    pub element: String,
    /// "value", "2s", "1s" or "lod".
    pub kind: String,
    pub unit: Option<String>,
    pub oxide: Option<String>,
    pub prop: bool,
    pub index: usize,
    pub header: String,
}

static REST_RE: OnceLock<Regex> = OnceLock::new();
static LOD_RE: OnceLock<Regex> = OnceLock::new();
static S2_RE: OnceLock<Regex> = OnceLock::new();
static S1_RE: OnceLock<Regex> = OnceLock::new();
static WT_RE: OnceLock<Regex> = OnceLock::new();
static FINAL_RE: OnceLock<Regex> = OnceLock::new();

const ALL_OXIDES: [&str; 24] = [
    "P2O5", "SiO2", "CaO", "Y2O3", "SO3", "La2O3", "Ce2O3", "Pr2O3", "Nd2O3", "Sm2O3", "Eu2O3", "Gd2O3", "Tb2O3", "Dy2O3",
    "Ho2O3", "Er2O3", "Tm2O3", "Yb2O3", "Lu2O3", "ThO2", "UO2", "PbO", "Al2O3", "ZrO2",
];

pub fn parse_header(header: &str) -> Option<TraceColumn> {
    let t0 = header.trim();
    if t0.is_empty() {
        return None;
    }
    let text = re(&FINAL_RE, r"(?i)^final\s+").replace(t0, "").to_string();
    let mut oxides: Vec<&str> = ALL_OXIDES.to_vec();
    oxides.sort_by_key(|o| std::cmp::Reverse(o.len()));
    let mut oxide = None;
    let mut rest = String::new();
    for ox in oxides {
        if let Some(r) = text.strip_prefix(ox) {
            if r.is_empty() || !r.chars().next().unwrap().is_alphanumeric() {
                oxide = Some(ox.to_string());
                rest = r.to_string();
                break;
            }
        }
    }
    let element = if let Some(ox) = &oxide {
        let s = element_symbol(ox);
        if s == "?" {
            return None;
        }
        s.to_string()
    } else {
        // ^([A-Z][a-z]?)(\d{1,3})?(?![a-z])(.*)$
        let c: Vec<char> = text.chars().collect();
        if c.is_empty() || !c[0].is_ascii_uppercase() {
            return None;
        }
        let mut sym = c[0].to_string();
        let mut i = 1;
        let try_two = c.get(1).is_some_and(|x| x.is_ascii_lowercase());
        // regex backtracking: prefer two-letter symbol, else one-letter
        let parse_from = |i: usize| -> Option<usize> {
            let mut j = i;
            let mut nd = 0;
            while j < c.len() && c[j].is_ascii_digit() && nd < 3 {
                j += 1;
                nd += 1;
            }
            // (?![a-z]) after optional digits; regex may also try fewer digits
            loop {
                if c.get(j).is_none_or(|x| !x.is_ascii_lowercase()) {
                    return Some(j);
                }
                if j == i {
                    return None;
                }
                j -= 1;
            }
        };
        let mut end = None;
        if try_two {
            if let Some(j) = parse_from(2) {
                sym.push(c[1]);
                i = 2;
                end = Some(j);
            }
        }
        if end.is_none() {
            end = parse_from(1);
            if end.is_none() {
                return None;
            }
        }
        let _ = i;
        if !is_element(&sym) {
            return None;
        }
        rest = c[end.unwrap()..].iter().collect();
        sym
    };
    let low = rest.to_lowercase();
    let rest_re = re(
        &REST_RE,
        r"ppm|ppb|ppt|wt\s*%|µg/g|ug/g|mean|median|avg|average|conc\.?|\(int\)|\(prop\)|int|prop|2\s*se|2\s*sd|2\s*s|2σ|1\s*se|1\s*sd|1σ|se|sd|σ|lod|d\.\s*l\.|dl|±|[\s_\-().:/]",
    );
    if !rest_re.replace_all(&low, "").is_empty() {
        return None;
    }
    let kind = if re(&LOD_RE, r"lod|d\.\s*l\.|\bdl\b|_dl").is_match(&low) {
        "lod"
    } else if re(&S2_RE, r"2\s*s|2σ").is_match(&low) {
        "2s"
    } else if re(&S1_RE, r"1\s*s|\bse\b|\bsd\b|_se|_sd|σ").is_match(&low) {
        "1s"
    } else {
        "value"
    };
    let mut unit = if low.contains("ppm") || low.contains("µg/g") || low.contains("ug/g") {
        Some("ppm")
    } else if low.contains("ppb") {
        Some("ppb")
    } else if low.contains("ppt") {
        Some("ppt")
    } else if re(&WT_RE, r"wt\s*%").is_match(&low) {
        Some("wt%")
    } else {
        None
    };
    if oxide.is_some() && unit.is_none() {
        unit = Some("wt%");
    }
    Some(TraceColumn {
        element,
        kind: kind.into(),
        unit: unit.map(String::from),
        oxide,
        prop: low.contains("prop"),
        index: 0,
        header: header.to_string(),
    })
}

static BDL_RE: OnceLock<Regex> = OnceLock::new();
static MISSING_RE: OnceLock<Regex> = OnceLock::new();

/// (number, status) with status "value", "bdl" or "missing".
pub fn parse_cell(v: &str) -> (Option<f64>, &'static str) {
    let t = v.trim();
    if re(&MISSING_RE, r"(?i)^(n\.?\s*d\.?|nd|na|n/a|-+|—|)$").is_match(t) {
        return (None, "missing");
    }
    if re(&BDL_RE, r"(?i)^(b\.?\s*d\.?\s*l\.?|bdl|<\s*lod|below\s*lod|<\s*dl|<.*)$").is_match(t) {
        return (None, "bdl");
    }
    match t.replace(',', ".").parse::<f64>() {
        Ok(x) if x.is_finite() => (Some(x), "value"),
        Ok(_) => (None, "missing"),
        Err(_) => (None, "missing"),
    }
}

pub fn detect_header_row(table: &[Vec<String>]) -> usize {
    for (i, row) in table.iter().take(50).enumerate() {
        if row.iter().filter(|c| parse_header(c).is_some()).count() >= 2 {
            return i;
        }
    }
    0
}

pub fn guess_label_column(header: &[String]) -> usize {
    header
        .iter()
        .position(|h| matches!(h.trim().to_lowercase().as_str(), "label" | "sample" | "name" | "spot" | "analysis" | "point"))
        .unwrap_or(0)
}

pub fn plan_columns(header: &[String], label_col: usize) -> (Vec<TraceColumn>, Vec<String>) {
    let mut cols = Vec::new();
    let mut ignored = Vec::new();
    for (i, h) in header.iter().enumerate() {
        if i == label_col {
            continue;
        }
        match parse_header(h) {
            None => {
                if !h.trim().is_empty() {
                    ignored.push(h.clone());
                }
            }
            Some(mut c) => {
                c.index = i;
                cols.push(c);
            }
        }
    }
    let prop: Vec<(String, String)> = cols.iter().filter(|c| c.prop).map(|c| (c.element.clone(), c.kind.clone())).collect();
    let mut keep: Vec<TraceColumn> = Vec::new();
    for c in cols {
        if (c.kind == "2s" || c.kind == "1s") && !c.prop && prop.contains(&(c.element.clone(), c.kind.clone())) {
            ignored.push(format!("{} (2SE(prop) used instead)", c.header));
            continue;
        }
        if keep.iter().any(|k| k.element == c.element && k.kind == c.kind) {
            ignored.push(format!("{} (duplicate)", c.header));
            continue;
        }
        keep.push(c);
    }
    (keep, ignored)
}

fn unit_factor(u: &str) -> f64 {
    match u {
        "ppb" => 1e-3,
        "ppt" => 1e-6,
        "wt%" => 1e4,
        _ => 1.0,
    }
}

fn to_ppm(v: f64, c: &TraceColumn, default_unit: &str) -> f64 {
    let unit = c.unit.as_deref().unwrap_or(default_unit);
    let mut ppm = v * unit_factor(unit);
    if let Some(ox) = &c.oxide {
        if oxide_molar_mass(ox).is_some() {
            ppm *= oxide_element_fraction(ox);
        }
    }
    ppm
}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub ppm: Option<f64>,
    pub s2: Option<f64>,
    pub lod: Option<f64>,
    pub bdl: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TraceRecord {
    pub label: String,
    pub key: String,
    pub values: BTreeMap<String, Entry>,
}

#[derive(Clone, Debug, Default)]
pub struct TraceImport {
    pub records: Vec<TraceRecord>,
    pub duplicates: Vec<String>,
    pub bdl_counts: BTreeMap<String, usize>,
    pub elements: Vec<String>,
}

pub fn build_records(table: &[Vec<String>], header_row: usize, label_col: usize, cols: &[TraceColumn], default_unit: &str) -> TraceImport {
    let mut out = TraceImport::default();
    let mut seen: Vec<String> = Vec::new();
    for row in table.iter().skip(header_row + 1) {
        let Some(label) = row.get(label_col).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else { continue };
        let mut values: BTreeMap<String, Entry> = BTreeMap::new();
        let mut any = false;
        for c in cols {
            let raw = row.get(c.index).map(|s| s.as_str()).unwrap_or("");
            let (number, status) = parse_cell(raw);
            if status != "missing" {
                any = true;
            }
            let e = values.entry(c.element.clone()).or_default();
            if c.kind == "value" {
                if status == "bdl" {
                    e.bdl = true;
                } else if let Some(x) = number {
                    e.ppm = Some(to_ppm(x, c, default_unit));
                }
            } else if let Some(x) = number {
                let p = to_ppm(x, c, default_unit);
                match c.kind.as_str() {
                    "1s" => e.s2 = Some(2.0 * p),
                    "2s" => e.s2 = Some(p),
                    _ => e.lod = Some(p),
                }
            }
        }
        if !any {
            continue;
        }
        let key = normalize_label(&label);
        if seen.contains(&key) {
            out.duplicates.push(label);
            continue;
        }
        seen.push(key.clone());
        out.records.push(TraceRecord { label, key, values });
    }
    out.elements = canonical_order(cols.iter().filter(|c| c.kind == "value").map(|c| c.element.clone()));
    let measured: Vec<String> = out
        .elements
        .iter()
        .filter(|el| out.records.iter().any(|r| r.values.get(*el).and_then(|e| e.ppm).unwrap_or(0.0) > 0.0))
        .cloned()
        .collect();
    for r in out.records.iter_mut() {
        for (el, e) in r.values.iter_mut() {
            if !measured.contains(el) && !e.bdl {
                continue;
            }
            let bad = e.bdl || e.ppm.is_none_or(|p| p <= 0.0) || matches!((e.ppm, e.lod), (Some(p), Some(l)) if p < l);
            if bad {
                e.bdl = true;
                *out.bdl_counts.entry(el.clone()).or_default() += 1;
            }
        }
    }
    out
}

/// Matches records to rows by normalised label: (row index -> record index, unmatched record indices).
/// How laser labels are matched to EPMA labels, beyond case and leading
/// zeros: text replacements applied to the laser label first (e.g. "_mnz" by
/// "-"), and optionally ignoring separators (- _ . and spaces) on both sides.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LabelRules {
    pub ignore_separators: bool,
    /// (find, replace), applied in order, case-insensitive.
    pub replace: Vec<(String, String)>,
}

impl LabelRules {
    pub fn is_plain(&self) -> bool {
        !self.ignore_separators && self.replace.iter().all(|(f, _)| f.is_empty())
    }
    fn squash(&self, s: String) -> String {
        if self.ignore_separators {
            s.chars().filter(|c| !matches!(c, '-' | '_' | '.' | ' ')).collect()
        } else {
            s
        }
    }
    /// Key of a laser (or imported) label.
    pub fn laser_key(&self, label: &str) -> String {
        let mut l = label.to_string();
        for (find, by) in &self.replace {
            if find.is_empty() {
                continue;
            }
            let lower = l.to_lowercase();
            let f = find.to_lowercase();
            let mut out = String::new();
            let mut last = 0;
            for (i, _) in lower.match_indices(&f) {
                out.push_str(&l[last..i]);
                out.push_str(by);
                last = i + f.len();
            }
            out.push_str(&l[last..]);
            l = out;
        }
        self.squash(normalize_label(&l))
    }
    /// Key of an EPMA label.
    pub fn epma_key(&self, label: &str) -> String {
        self.squash(normalize_label(label))
    }
}

/// Matches imported or laser records to rows by label, with the lab's rules.
pub fn match_labels(rows: &[Row], labels: &[String], rules: &LabelRules) -> (BTreeMap<usize, usize>, Vec<usize>) {
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        index.entry(rules.epma_key(&text(r, "Label"))).or_insert(i);
    }
    let mut matched = BTreeMap::new();
    let mut unmatched = Vec::new();
    for (ri, l) in labels.iter().enumerate() {
        match index.get(&rules.laser_key(l)) {
            Some(i) => {
                matched.insert(*i, ri);
            }
            None => unmatched.push(ri),
        }
    }
    (matched, unmatched)
}

pub fn match_records(rows: &[Row], keys: &[String]) -> (BTreeMap<usize, usize>, Vec<usize>) {
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        index.entry(normalize_label(&text(r, "Label"))).or_insert(i);
    }
    let mut matched = BTreeMap::new();
    let mut unmatched = Vec::new();
    for (ri, k) in keys.iter().enumerate() {
        match index.get(k) {
            Some(i) => {
                matched.insert(*i, ri);
            }
            None => unmatched.push(ri),
        }
    }
    (matched, unmatched)
}

pub fn trace_update_for_row(row: &Row, rec: &TraceRecord, source: &str) -> Row {
    let mut up = Row::new();
    let mut flags: Vec<String> = te_below_dl(row).into_iter().filter(|e| !rec.values.contains_key(e)).collect();
    for (el, e) in &rec.values {
        set(&mut up, &format!("{el}_ppm"), jopt(e.ppm));
        set(&mut up, &format!("{el}_2s_ppm"), jopt(e.s2));
        set(&mut up, &format!("{el}_LOD_ppm"), jopt(e.lod));
        if e.bdl {
            flags.push(el.clone());
        }
    }
    set(&mut up, TE_BELOW_DL, jstr(canonical_order(flags).join(",")));
    let mut sources: Vec<String> = text(row, TE_SOURCE).split("; ").filter(|s| !s.is_empty()).map(String::from).collect();
    if !sources.iter().any(|s| s == source) {
        sources.push(source.to_string());
    }
    set(&mut up, TE_SOURCE, jstr(sources.join("; ")));
    up
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers() {
        let h = parse_header("Lu175_ppm_mean").unwrap();
        assert_eq!((h.element.as_str(), h.kind.as_str(), h.unit.as_deref()), ("Lu", "value", Some("ppm")));
        let h = parse_header("Lu175_ppm_2SE(prop)").unwrap();
        assert_eq!((h.kind.as_str(), h.prop), ("2s", true));
        assert_eq!(parse_header("Lu175_LOD").unwrap().kind, "lod");
        assert_eq!(parse_header("La2O3").unwrap().unit.as_deref(), Some("wt%"));
        assert!(parse_header("Label").is_none());
        assert!(parse_header("Comment").is_none());
        assert_eq!(parse_header("Y (ppm)").unwrap().element, "Y");
    }
    #[test]
    fn labels() {
        assert_eq!(normalize_label("S1B2_mnz45_02"), normalize_label("s1b2_mnz45_2"));
        assert_eq!(grain_key("s1b4_mnz26_3"), "s1b4_mnz26");
        assert_eq!(parse_cell("b.d.l.").1, "bdl");
        assert_eq!(parse_cell("<0.2").1, "bdl");
        assert_eq!(parse_cell("1,5").0, Some(1.5));
    }
}
