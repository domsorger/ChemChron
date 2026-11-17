// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading EPMA data: raw instrument exports ("Point,Comment,...(Mass%)")
//! and ChemChron CSV tables, plus pasted tables.

use crate::constants::*;
use crate::processing::{process_row, Context};
use crate::row::*;
use crate::sessions::apply_detection_rule;
use crate::table::{read_csv_rows, split_csv_line};
use std::path::Path;

const ERR_ELEMENT: [(&str, &str); 3] = [("ThO2", "Th"), ("UO2", "U"), ("PbO", "Pb")];

fn number_or_none(s: Option<&str>) -> Option<f64> {
    let t = s?.trim();
    if t.is_empty() {
        return None;
    }
    parse_locale(t).ok().flatten()
}

fn field_str(row: &Row, key: &str) -> Option<String> {
    row.get(key).map(|v| value_text(Some(v)))
}

/// 2s (oxide wt%) and detection limit (oxide wt%) from instrument columns.
pub fn oxide_uncertainty_fields(oxide: &str, value: Option<f64>, err_pct: Option<f64>, dl_ppm: Option<f64>) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let (Some(e), Some(v)) = (err_pct, value) {
        out.push((format!("{oxide}_2s_wt%"), 2.0 * v.abs() * e / 100.0));
    }
    if let Some(dl) = dl_ppm {
        if oxide_molar_mass(oxide).is_some() {
            out.push((format!("{oxide}_DL_wt%"), dl / 1e4 / oxide_element_fraction(oxide)));
        }
    }
    out
}

/// Instrument columns in a ChemChron CSV row: Error% to sigma, D.L. to Below_DL.
pub fn apply_instrument_columns(row: &mut Row, active: &[String]) {
    for (ox, el) in ERR_ELEMENT {
        if number_or_none(field_str(row, &format!("sigma_{ox}")).as_deref()).is_some() {
            continue;
        }
        let e = number_or_none(field_str(row, &format!("{el}(Error%)")).as_deref());
        let v = number_or_none(field_str(row, ox).as_deref());
        if let (Some(e), Some(v)) = (e, v) {
            set(row, &format!("sigma_{ox}"), jnum(v.abs() * e / 100.0));
        }
    }
    let mut flags = crate::sessions::below_dl_set(row);
    let mut all: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
    all.extend(active.iter().cloned());
    for ox in all {
        let el = element_symbol(&ox);
        let el = if el == "?" { None } else { Some(el) };
        let dl = el.and_then(|e| number_or_none(field_str(row, &format!("{e}(D.L.)")).as_deref()));
        let v = number_or_none(field_str(row, &ox).as_deref());
        let e = el.and_then(|e| number_or_none(field_str(row, &format!("{e}(Error%)")).as_deref()));
        for (k, x) in oxide_uncertainty_fields(&ox, v, e, dl) {
            if number_or_none(field_str(row, &k).as_deref()).is_none() {
                set(row, &k, jnum(x));
            }
        }
        let (Some(dl), Some(v)) = (dl, v) else { continue };
        if flags.contains(&ox) {
            continue;
        }
        if v * 1e4 * oxide_element_fraction(&ox) < dl {
            flags.push(ox.clone());
        }
    }
    if !flags.is_empty() {
        set(row, "Below_DL", jstr(flags.join(",")));
    }
}

pub fn looks_like_epma_export(text: &str) -> bool {
    text.lines().take(11).any(|l| l.trim_start_matches('\u{feff}').starts_with("Point,Comment,") && l.contains("(Mass%)"))
}

pub fn detect_active_oxides(header: &[String]) -> Vec<String> {
    let present: Vec<String> = header.iter().filter_map(|c| c.trim().strip_suffix("(Mass%)").map(String::from)).collect();
    optional_oxides_master().into_iter().filter(|o| present.iter().any(|p| p == o)).map(String::from).collect()
}

pub struct Parsed {
    pub rows: Vec<Row>,
    pub active: Vec<String>,
    pub skipped: Vec<String>,
}

/// Raw multi-sample instrument export.
pub fn parse_epma_export(text: &str) -> Parsed {
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    let mut current: Option<String> = None;
    let mut header: Option<Vec<String>> = None;
    let mut active: Vec<String> = Vec::new();
    for raw in read_csv_rows(text, ',') {
        if raw.is_empty() || (raw.len() == 1 && raw[0].is_empty()) {
            continue;
        }
        let joined = raw.join(",");
        if joined.contains("Sample:") {
            for cell in &raw {
                if cell.trim().starts_with("Sample:") {
                    current = Some(cell.split_once(':').map(|x| x.1.trim().to_string()).unwrap_or_default());
                }
            }
            header = None;
            continue;
        }
        if raw[0].trim() == "Point" && raw.len() > 1 && raw[1].trim() == "Comment" {
            let h: Vec<String> = raw.iter().map(|s| s.trim().to_string()).collect();
            for ox in detect_active_oxides(&h) {
                if !active.contains(&ox) {
                    active.push(ox);
                }
            }
            header = Some(h);
            continue;
        }
        let Some(h) = &header else { continue };
        let point = raw[0].trim();
        let is_int = {
            let p = point.strip_prefix('-').unwrap_or(point);
            !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())
        };
        if !is_int {
            continue;
        }
        let idx = |name: &str| h.iter().position(|x| x == name);
        let get = |name: &str| -> Option<f64> {
            let i = idx(name)?;
            let v = raw.get(i)?.trim();
            if v.is_empty() {
                return None;
            }
            parse_locale(v).ok().flatten()
        };
        let get_req = |name: &str| -> Result<f64, String> {
            let Some(i) = idx(name).filter(|i| *i < raw.len()) else { return Err(format!("{name} column not found")) };
            let t = raw[i].trim();
            if t.is_empty() {
                return Err(format!("{name} is empty"));
            }
            parse_locale(t).ok().flatten().ok_or_else(|| format!("{name} value '{t}' could not be parsed as a number"))
        };
        let ci = idx("Comment").unwrap_or(1);
        let comment = raw.get(ci).map(|s| s.trim().to_string()).unwrap_or_default();
        let label = if comment.is_empty() { format!("{}_{point}", current.clone().unwrap_or_else(|| "sample".into())) } else { comment };
        let th = get_req("ThO2(Mass%)");
        let u = get_req("UO2(Mass%)");
        let pb = get_req("PbO(Mass%)");
        let (th, u, pb) = match (th, u, pb) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (a, b, c) => {
                let reason = a.err().or(b.err()).or(c.err()).unwrap_or_default();
                skipped.push(format!("{label} (point {point}): {reason}"));
                continue;
            }
        };
        let mut row = Row::new();
        set(&mut row, "Label", jstr(&label));
        set(&mut row, "ThO2", jnum(th));
        set(&mut row, "UO2", jnum(u));
        set(&mut row, "PbO", jnum(pb));
        set(&mut row, "SampleGroup", jstr(current.clone().unwrap_or_default()));
        for ox in &active {
            if let Some(v) = get(&format!("{ox}(Mass%)")) {
                set(&mut row, ox, jnum(v));
            }
        }
        for (ox, el) in ERR_ELEMENT {
            if let Some(e) = get(&format!("{el}(Error%)")) {
                let b = match ox {
                    "ThO2" => th,
                    "UO2" => u,
                    _ => pb,
                };
                set(&mut row, &format!("sigma_{ox}"), jnum(b.abs() * e / 100.0));
            }
        }
        let mut below = Vec::new();
        let mut all: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
        all.extend(active.iter().cloned());
        for ox in all {
            let el = element_symbol(&ox);
            if el == "?" {
                continue;
            }
            let dl = get(&format!("{el}(D.L.)"));
            let measured = num(&row, &ox);
            for (k, x) in oxide_uncertainty_fields(&ox, measured, get(&format!("{el}(Error%)")), dl) {
                set(&mut row, &k, jnum(x));
            }
            let (Some(dl), Some(m)) = (dl, measured) else { continue };
            if m * 1e4 * oxide_element_fraction(&ox) < dl {
                below.push(ox);
            }
        }
        if !below.is_empty() {
            set(&mut row, "Below_DL", jstr(below.join(",")));
        }
        rows.push(row);
    }
    Parsed { rows, active, skipped }
}

pub struct Imported {
    pub results: Vec<Row>,
    pub active: Vec<String>,
    pub skipped: Vec<String>,
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// Reads one EPMA file (instrument export or ChemChron CSV) and processes every row.
pub fn process_csv_text<'a>(input: &str, name: &str, ctx_for: &dyn Fn(&[String]) -> Context<'a>) -> Result<Imported, String> {
    let body = input.trim_start_matches('\u{feff}');
    if looks_like_epma_export(body) {
        let p = parse_epma_export(body);
        if p.rows.is_empty() {
            return Err(format!(
                "{name}: no usable Th/U/Pb data points found (check ThO2(Mass%), UO2(Mass%), PbO(Mass%) columns)."
            ));
        }
        let ctx = ctx_for(&p.active);
        let mut results = Vec::new();
        for r in &p.rows {
            results.push(process_row(r, &ctx)?);
        }
        let mut ox: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
        ox.extend(p.active.iter().cloned());
        apply_detection_rule(&mut results, &ox);
        return Ok(Imported { results, active: p.active, skipped: p.skipped });
    }
    let table = read_csv_rows(body, ',');
    let Some(header) = table.first() else { return Err(format!("{name}: the file is empty")) };
    let header: Vec<String> = header.iter().map(|s| s.to_string()).collect();
    let missing: Vec<&str> = REQUIRED_COLUMNS.iter().copied().filter(|c| !header.iter().any(|h| h == c)).collect();
    if !missing.is_empty() {
        return Err(format!("{name}: missing required columns {missing:?}"));
    }
    let active: Vec<String> =
        optional_oxides_master().into_iter().filter(|o| header.iter().any(|h| h == o)).map(String::from).collect();
    let ctx = ctx_for(&active);
    let mut results = Vec::new();
    let mut skipped = Vec::new();
    let mut n_fallback = 0;
    for (i, cells) in table.iter().enumerate().skip(1) {
        if cells.iter().all(|c| c.is_empty()) && cells.len() <= 1 {
            continue;
        }
        let line_no = i + 1;
        let mut row = Row::new();
        for (j, h) in header.iter().enumerate() {
            set(&mut row, h, jstr(cells.get(j).cloned().unwrap_or_default()));
        }
        let label = {
            let l = text(&row, "Label").trim().to_string();
            if l.is_empty() { format!("row at line {line_no}") } else { l }
        };
        apply_instrument_columns(&mut row, &active);
        if ERR_ELEMENT.iter().any(|(ox, _)| number_or_none(field_str(&row, &format!("sigma_{ox}")).as_deref()).is_none()) {
            n_fallback += 1;
        }
        match process_row(&row, &ctx) {
            Ok(r) => results.push(r),
            Err(e) => skipped.push(format!("{label} (line {line_no}): {e}")),
        }
    }
    if n_fallback > 0 {
        skipped.push(format!(
            "note: {n_fallback} row(s) have no measured Th/U/Pb error (sigma_ThO2/UO2/PbO or Th/U/Pb(Error%) columns); fallback relative errors were used for their ages"
        ));
    }
    let mut ox: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
    ox.extend(active.iter().cloned());
    apply_detection_rule(&mut results, &ox);
    Ok(Imported { results, active, skipped })
}

pub fn process_csv_file<'a>(path: &Path, ctx_for: &dyn Fn(&[String]) -> Context<'a>) -> Result<Imported, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    process_csv_text(&text, &file_name(path), ctx_for)
}

/// A pasted table (tab, ";" or "," separated), decimal commas converted.
pub fn clipboard_table(text: &str) -> Vec<Vec<String>> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = norm.split('\n').filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let first = lines[0];
    let delim = if first.contains('\t') {
        '\t'
    } else if first.matches(';').count() > first.matches(',').count() {
        ';'
    } else {
        ','
    };
    let mut rows: Vec<Vec<String>> = lines.iter().map(|l| split_csv_line(l, delim).into_iter().map(|c| c.trim().to_string()).collect()).collect();
    if delim != ',' {
        for r in rows.iter_mut() {
            for c in r.iter_mut() {
                if is_decimal_comma(c) {
                    *c = c.replace(',', ".");
                }
            }
        }
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for r in rows.iter_mut() {
        r.resize(width, String::new());
    }
    rows
}

fn is_decimal_comma(c: &str) -> bool {
    let t = c.strip_prefix(['+', '-']).unwrap_or(c);
    match t.split_once(',') {
        Some((a, b)) => !a.is_empty() && !b.is_empty() && a.chars().all(|x| x.is_ascii_digit()) && b.chars().all(|x| x.is_ascii_digit()),
        None => false,
    }
}

/// The pasted table as CSV text that the importer reads.
pub fn table_to_csv(table: &[Vec<String>]) -> String {
    let mut s = String::new();
    for r in table {
        let cells: Vec<String> = r.iter().map(|c| crate::table::csv_escape(c)).collect();
        s.push_str(&cells.join(","));
        s.push_str("\r\n");
    }
    s
}

/// ("export" | "chemchron" | "", used, ignored) for a pasted header.
pub fn describe_pasted_columns(header: &[String]) -> (String, Vec<String>, Vec<String>) {
    let h: Vec<String> = header.iter().map(|s| s.trim().to_string()).collect();
    let mut oxides: Vec<&str> = PARENT_OXIDES.to_vec();
    oxides.extend(optional_oxides_master());
    let elements: Vec<&str> = oxides.iter().map(|o| element_symbol(o)).collect();
    let strip_err = |x: &str| -> Option<String> {
        x.strip_suffix("(Error%)").or_else(|| x.strip_suffix("(D.L.)")).map(String::from)
    };
    let plain = ["Label", "Comment", "Flag", "SampleGroup", "Reference", "Is_Reference", "Include_In_Average", "Mineral", "Below_DL", "EPMA_Session"];
    let fmt;
    let used: Box<dyn Fn(&str) -> bool> = if h.len() >= 2 && h[0] == "Point" && h[1] == "Comment" && h.iter().any(|x| x.ends_with("(Mass%)")) {
        fmt = "export";
        Box::new(move |x: &str| {
            x == "Point"
                || x == "Comment"
                || x.strip_suffix("(Mass%)").is_some_and(|o| oxides.contains(&o))
                || strip_err(x).is_some_and(|e| elements.contains(&e.as_str()))
        })
    } else if ["Label", "ThO2", "UO2", "PbO"].iter().all(|c| h.iter().any(|x| x == c)) {
        fmt = "chemchron";
        Box::new(move |x: &str| {
            plain.contains(&x)
                || oxides.contains(&x)
                || x.strip_prefix("sigma_").is_some_and(|o| oxides.contains(&o))
                || strip_err(x).is_some_and(|e| elements.contains(&e.as_str()))
                || x.ends_with("_2s_wt%")
                || x.ends_with("_DL_wt%")
        })
    } else {
        return (String::new(), Vec::new(), h);
    };
    let u: Vec<String> = h.iter().filter(|x| !x.is_empty() && used(x)).cloned().collect();
    let ig: Vec<String> = h.iter().filter(|x| !x.is_empty() && !used(x)).cloned().collect();
    (fmt.into(), u, ig)
}
