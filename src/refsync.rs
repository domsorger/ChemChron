// SPDX-License-Identifier: GPL-3.0-or-later
//! One reference-material list for the whole lab. It lives in
//! `reference_materials.json` in the settings folder:
//! `{mineral: {code: entry}}`. An entry keeps its EPMA fields (name,
//! chemical age, 2σ, source, U–Pb accepted ages) and, under `"laser"`, what
//! the Laser part needs (its name, label aliases, matrix, source, checked,
//! and the accepted ratios / element contents). Glasses that only the laser
//! uses sit under `"Glass"`.
//!
//! The open laser session always uses this list: a newly opened session adds
//! materials the list doesn't have yet and then takes the list's values;
//! edits on LA-ICP-MS > Reference materials are written back to the list, and
//! changes made on Settings > Reference materials reach the laser session.

use crate::app::ChemApp;
use chemchron_core::constants::MINERAL_NAMES;
use serde_json::{json, Map, Value};
use chemchron_laser_core::refmat::{ReferenceMaterial, RmValue};

pub const GLASS: &str = "Glass";

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Sections of the lab list: the EPMA minerals, other matrices (apatite,
/// titanite, …) and glasses last.
pub fn sections(nested: &Map<String, Value>) -> Vec<String> {
    let mut v: Vec<String> = MINERAL_NAMES.iter().map(|m| m.to_string()).collect();
    let mut other: Vec<String> = nested.keys().filter(|k| !v.contains(k) && k.as_str() != GLASS).cloned().collect();
    other.sort();
    v.extend(other);
    v.push(GLASS.to_string());
    v
}

/// Laser materials held in the lab list, in list order.
pub fn laser_list(nested: &Map<String, Value>) -> Vec<ReferenceMaterial> {
    let mut out = Vec::new();
    for sec in sections(nested) {
        let Some(Value::Object(entries)) = nested.get(&sec) else { continue };
        for (code, e) in entries {
            let Some(l) = e.get("laser").and_then(|l| l.as_object()) else { continue };
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or(code).to_string();
            let aliases: Vec<String> = chemchron_core::reference::codes_of(code, e).into_iter().filter(|a| squash(a) != squash(&name)).collect();
            let values = l
                .get("values")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            let value = v.get("value")?.as_f64()?;
                            Some((k.clone(), RmValue { value, unc2s: v.get("unc2s").and_then(|u| u.as_f64()).unwrap_or(0.0) }))
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(ReferenceMaterial {
                name,
                aliases,
                matrix: l.get("matrix").and_then(|v| v.as_str()).unwrap_or(if sec == GLASS { "Glass" } else { &sec }).to_string(),
                source: l.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                confirmed: l.get("confirmed").and_then(|v| v.as_bool()).unwrap_or(false),
                values,
            });
        }
    }
    out
}

fn laser_value(m: &ReferenceMaterial, code: &str) -> Value {
    let aliases: Vec<&String> = m.aliases.iter().filter(|a| squash(a) != squash(code)).collect();
    let values: Map<String, Value> = m.values.iter().map(|(k, v)| (k.clone(), json!({"value": v.value, "unc2s": v.unc2s}))).collect();
    json!({"name": m.name, "aliases": aliases, "matrix": m.matrix, "source": m.source, "confirmed": m.confirmed, "values": values})
}

/// (section, code) of the lab entry a laser material belongs to.
fn find_entry(nested: &Map<String, Value>, m: &ReferenceMaterial, prefixes: &[(String, String)]) -> Option<(String, String)> {
    let names: Vec<String> = std::iter::once(&m.name).chain(m.aliases.iter()).map(|n| squash(n)).collect();
    // 1. already linked by its laser name
    for sec in sections(nested) {
        if let Some(Value::Object(entries)) = nested.get(&sec) {
            for (code, e) in entries {
                if e.get("laser").and_then(|l| l.get("name")).and_then(|n| n.as_str()).is_some_and(|n| squash(n) == squash(&m.name)) {
                    return Some((sec.to_string(), code.clone()));
                }
            }
        }
    }
    // 2. a code equal to its name or an alias, 3. a U–Pb label prefix pointing to a code
    let mut wanted: Vec<String> = names.clone();
    for (p, c) in prefixes {
        if names.contains(&squash(p)) {
            wanted.push(squash(c));
        }
    }
    for w in wanted {
        for sec in sections(nested) {
            if let Some(Value::Object(entries)) = nested.get(&sec) {
                for code in entries.keys() {
                    if squash(code) == w {
                        return Some((sec.to_string(), code.clone()));
                    }
                }
            }
        }
    }
    None
}

fn section_for(matrix: &str) -> String {
    let m = matrix.trim();
    if let Some(x) = MINERAL_NAMES.iter().find(|x| x.eq_ignore_ascii_case(m)) {
        return x.to_string();
    }
    if m.is_empty() || m.to_lowercase().contains("glass") {
        return GLASS.to_string();
    }
    if m.eq_ignore_ascii_case("CaCO3") || m.to_lowercase().contains("carbonate") {
        return "Carbonate".to_string();
    }
    let mut c = m.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Writes laser materials into the lab list (`only_missing`: keep values already there).
pub fn merge_into_lab(nested: &mut Map<String, Value>, list: &[ReferenceMaterial], prefixes: &[(String, String)], only_missing: bool) -> usize {
    let mut n = 0;
    for m in list {
        let (sec, code) = find_entry(nested, m, prefixes).unwrap_or_else(|| (section_for(&m.matrix), m.name.clone()));
        let entries = nested.entry(sec).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(entries) = entries else { continue };
        let e = entries.entry(code.clone()).or_insert_with(|| json!({"name": m.name, "age": "", "age_2s": "", "source": ""}));
        let Value::Object(e) = e else { continue };
        if only_missing && e.contains_key("laser") {
            continue;
        }
        let mut new = laser_value(m, &code);
        // alternative codes kept on the entry itself are not repeated under "laser"
        let top: Vec<String> = e.get("aliases").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(squash)).collect()).unwrap_or_default();
        if let Some(Value::Array(a)) = new.get_mut("aliases") {
            a.retain(|x| x.as_str().is_none_or(|x| !top.contains(&squash(x))));
        }
        if e.get("laser") != Some(&new) {
            e.insert("laser".into(), new);
            n += 1;
        }
    }
    n
}

/// Removes the laser part of lab entries whose material is no longer in `list`.
fn drop_missing(nested: &mut Map<String, Value>, list: &[ReferenceMaterial]) {
    for sec in sections(nested) {
        if let Some(Value::Object(entries)) = nested.get_mut(&sec) {
            let codes: Vec<String> = entries.keys().cloned().collect();
            for code in codes {
                let name = entries[&code].get("laser").and_then(|l| l.get("name")).and_then(|n| n.as_str()).map(String::from);
                if let Some(name) = name {
                    if !list.iter().any(|m| m.name == name) {
                        if let Some(Value::Object(e)) = entries.get_mut(&code) {
                            e.remove("laser");
                        }
                        let laser_only = sec == GLASS && entries[&code].get("age").is_none_or(|a| a.as_f64().is_none());
                        if laser_only {
                            entries.remove(&code);
                        }
                    }
                }
            }
        }
    }
}

/// Reads reference-material files in JSON (`Properties` with the material's
/// name, matrix and data source; `Results` with each quantity's name, value,
/// uncertainty and units) and merges them into the lab list. A file joins the
/// entry whose code, alternative code or laser name equals its name (a
/// one-letter matrix prefix followed by `_` is ignored, `M_44069` matches
/// `44069`); otherwise it becomes a new entry. Values already in the list are
/// kept; missing ones are added, unchecked.
/// Returns (new entries, entries that got values, files that could not be read).
pub fn import_reference_files(nested: &mut Map<String, Value>, files: &[std::path::PathBuf]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut added, mut merged, mut bad) = (Vec::new(), Vec::new(), Vec::new());
    for path in files {
        let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let Some(v) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
            bad.push(fname);
            continue;
        };
        let props = v.get("Properties").cloned().unwrap_or(Value::Null);
        let Some(results) = v.get("Results").and_then(|r| r.as_array()) else {
            bad.push(fname);
            continue;
        };
        let pstr = |k: &str| props.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let file_name_rm = { let n = pstr("Name"); if n.is_empty() { fname.trim_end_matches(".json").to_string() } else { n } };
        // a one-letter matrix prefix followed by `_` is ignored
        let short = match file_name_rm.split_once('_') {
            Some((p, rest)) if p.len() == 1 && !rest.is_empty() => rest.to_string(),
            _ => file_name_rm.clone(),
        };
        let matrix = pstr("Matrix");
        let mut age: Option<(f64, f64)> = None;
        let mut values = Map::new();
        for r in results {
            let Some(name) = r.get("Name").and_then(|x| x.as_str()) else { continue };
            let Some(val) = r.get("Value").and_then(|x| x.as_f64()).filter(|x| x.is_finite()) else { continue };
            let unc = r.get("Uncertainty").and_then(|x| x.as_f64()).filter(|x| x.is_finite()).unwrap_or(0.0);
            let units = r.get("Units").and_then(|x| x.as_str()).unwrap_or("").to_lowercase();
            if name.eq_ignore_ascii_case("age") {
                if val > 0.0 {
                    age = Some((val, unc));
                }
                continue;
            }
            let (value, unc2s) = match units.as_str() {
                "wt%" | "wt %" => (val * 1e4, unc * 1e4),
                "ppb" => (val / 1e3, unc / 1e3),
                _ => (val, unc),
            };
            if units == "ratio" || units == "ppm" || units == "wt%" || units == "wt %" || units == "ppb" || name.contains('/') {
                values.insert(name.to_string(), json!({"value": value, "unc2s": unc2s}));
            }
        }
        // find the entry
        let wanted = [squash(&file_name_rm), squash(&short)];
        let mut found: Option<(String, String)> = None;
        'outer: for sec in sections(nested) {
            if let Some(Value::Object(entries)) = nested.get(&sec) {
                for (code, e) in entries {
                    if chemchron_core::reference::codes_of(code, e).iter().any(|c| wanted.contains(&squash(c))) {
                        found = Some((sec.clone(), code.clone()));
                        break 'outer;
                    }
                }
            }
        }
        // then a code of the same matrix named in the description ("Monazite 44069, natural standard")
        if found.is_none() {
            let sec = section_for(&matrix);
            if let Some(Value::Object(entries)) = nested.get(&sec) {
                let refs: chemchron_core::reference::Refs = entries.clone();
                if let Some(code) = chemchron_core::reference::guess_reference_from_label(&pstr("Description"), &refs) {
                    found = Some((sec.clone(), code));
                } else {
                    // or an entry whose name has the file's name as a word ("Grain 44069, Wilmington Complex")
                    let mut probe = chemchron_core::reference::Refs::new();
                    probe.insert(short.clone(), json!({}));
                    if let Some(code) = entries.iter().find(|(_, e)| e.get("name").and_then(|n| n.as_str()).is_some_and(|n| chemchron_core::reference::guess_reference_from_label(n, &probe).is_some())).map(|(c, _)| c.clone()) {
                        found = Some((sec, code));
                    }
                }
            }
        }
        let source = pstr("Data Source");
        let (sec, code) = match found {
            Some(x) => x,
            None => {
                let sec = section_for(&matrix);
                let entries = nested.entry(sec.clone()).or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(entries) = entries {
                    let desc = pstr("Description");
                    entries.insert(
                        short.clone(),
                        json!({"name": if desc.is_empty() { short.clone() } else { desc }, "age": "", "age_2s": "", "source": "", "aliases": []}),
                    );
                }
                added.push(short.clone());
                (sec, short.clone())
            }
        };
        let Some(Value::Object(e)) = nested.get_mut(&sec).and_then(|x| x.as_object_mut()).and_then(|m| m.get_mut(&code)) else { continue };
        if let Some((a, u)) = age {
            if e.get("age").and_then(|x| x.as_f64()).is_none() {
                e.insert("age".into(), json!(a));
                e.insert("age_2s".into(), json!(u));
                if e.get("source").and_then(|x| x.as_str()).is_none_or(|x| x.is_empty()) {
                    e.insert("source".into(), json!(source.clone()));
                }
            }
        }
        if squash(&file_name_rm) != squash(&code) {
            let list = e.entry("aliases").or_insert_with(|| json!([]));
            if let Value::Array(a) = list {
                if !a.iter().any(|x| x.as_str().is_some_and(|x| squash(x) == squash(&file_name_rm))) {
                    a.push(json!(file_name_rm.clone()));
                }
            }
        }
        if values.is_empty() {
            continue;
        }
        let laser = e.entry("laser").or_insert_with(|| json!({"name": short.clone(), "aliases": [], "matrix": if matrix.is_empty() { sec.clone() } else { matrix.clone() }, "source": source.clone(), "confirmed": false, "values": {}}));
        let Value::Object(l) = laser else { continue };
        let lv = l.entry("values").or_insert_with(|| json!({}));
        let Value::Object(lv) = lv else { continue };
        let mut n = 0;
        for (k, v) in values {
            if !lv.contains_key(&k) {
                lv.insert(k, v);
                n += 1;
            }
        }
        if n > 0 && !added.contains(&code) {
            merged.push(code.clone());
        }
    }
    (added, merged, bad)
}

fn session_key(app: &ChemApp) -> String {
    format!("{}|{:?}|{}", app.laser.session.name, app.laser.session.source, app.laser.session.files.len())
}

#[derive(Default)]
pub struct SyncState {
    pub session_key: String,
    pub session_json: String,
    pub lab_json: String,
}

/// Keeps the open laser session and the lab list in step (called every frame; cheap).
pub fn sync(app: &mut ChemApp) {
    let prefixes: Vec<(String, String)> = app.settings.label_codes();
    let key = session_key(app);
    let session_json = serde_json::to_string(&app.laser.session.rms).unwrap_or_default();
    let mut st = std::mem::take(&mut app.rm_state);
    let mut lab_changed = false;
    // The lab list gets the built-in starting set once.
    if laser_list(&app.settings.refs_nested).is_empty() {
        let builtin = chemchron_laser_core::refmat::builtin(&chemchron_laser_core::geochron::DecayConstants::default());
        merge_into_lab(&mut app.settings.refs_nested, &builtin, &prefixes, true);
        lab_changed = true;
    }
    if app.laser.is_demo() {
        // the simulated session keeps its own known truth
    } else if key != st.session_key {
        // A session was opened: it adds what the list lacks, then uses the list.
        if merge_into_lab(&mut app.settings.refs_nested, &app.laser.session.rms, &prefixes, true) > 0 {
            lab_changed = true;
        }
        apply_to_session(app);
    } else if session_json != st.session_json {
        // Edited on LA-ICP-MS > Reference materials.
        let list = app.laser.session.rms.clone();
        merge_into_lab(&mut app.settings.refs_nested, &list, &prefixes, false);
        drop_missing(&mut app.settings.refs_nested, &list);
        lab_changed = true;
    } else {
        let lab = serde_json::to_string(&laser_list(&app.settings.refs_nested)).unwrap_or_default();
        if lab != st.lab_json && !st.lab_json.is_empty() {
            // Edited on Settings > Reference materials.
            apply_to_session(app);
        }
    }
    if lab_changed {
        app.save_settings_quiet();
    }
    st.session_key = key;
    st.session_json = serde_json::to_string(&app.laser.session.rms).unwrap_or_default();
    st.lab_json = serde_json::to_string(&laser_list(&app.settings.refs_nested)).unwrap_or_default();
    app.rm_state = st;
}

fn apply_to_session(app: &mut ChemApp) {
    let list = laser_list(&app.settings.refs_nested);
    if list.is_empty() || serde_json::to_string(&list).ok() == serde_json::to_string(&app.laser.session.rms).ok() {
        return;
    }
    app.laser.session.rms = list;
    app.laser.recalculate();
}

/// Every code an entry answers to (code, alternative codes, laser name and aliases), squashed.
fn all_codes(code: &str, e: &Value) -> Vec<String> {
    let mut v: Vec<String> = chemchron_core::reference::codes_of(code, e).iter().map(|c| squash(c)).collect();
    if let Some(l) = e.get("laser") {
        if let Some(n) = l.get("name").and_then(|n| n.as_str()) {
            v.push(squash(n));
        }
        if let Some(a) = l.get("aliases").and_then(|a| a.as_array()) {
            v.extend(a.iter().filter_map(|x| x.as_str()).map(squash));
        }
    }
    v.retain(|c| !c.is_empty());
    v
}

/// The list shipped with the program, plus the entries of `current` it doesn't
/// have (materials added by the lab). Returns the new list and the codes kept.
pub fn with_shipped(current: &Map<String, Value>) -> (Map<String, Value>, Vec<String>) {
    let mut out = match serde_json::from_str::<Value>(chemchron_core::settings::SHIPPED_REFERENCE_MATERIALS) {
        Ok(Value::Object(o)) => o,
        _ => return (current.clone(), Vec::new()),
    };
    let known: Vec<String> = out.values().filter_map(|m| m.as_object()).flat_map(|m| m.iter().flat_map(|(c, e)| all_codes(c, e))).collect();
    let mut kept = Vec::new();
    for (sec, entries) in current {
        let Some(entries) = entries.as_object() else { continue };
        for (code, e) in entries {
            if all_codes(code, e).iter().any(|c| known.contains(c)) {
                continue;
            }
            if let Value::Object(m) = out.entry(sec.clone()).or_insert_with(|| Value::Object(Map::new())) {
                m.insert(code.clone(), e.clone());
                kept.push(code.clone());
            }
        }
    }
    for m in MINERAL_NAMES {
        out.entry(m.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    (out, kept)
}

/// The lab code of a laser reference material (its laser name in the reference list).
pub fn lab_code_of_laser_rm(nested: &Map<String, Value>, laser_name: &str) -> Option<String> {
    for entries in nested.values() {
        let Some(entries) = entries.as_object() else { continue };
        for (code, e) in entries {
            if e.get("laser").and_then(|l| l.get("name")).and_then(|n| n.as_str()) == Some(laser_name) {
                return Some(code.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_list_replaces_old_codes_and_keeps_lab_additions() {
        let current: Map<String, Value> = serde_json::from_value(json!({
            "Monazite": {"MBA": {"name": "old", "age": 500.0, "age_2s": 5.0, "source": ""},
                         "MyOwn": {"name": "lab grain", "age": 300.0, "age_2s": 4.0, "source": "in-house"}},
            "Glass": {"BCR2G": {"name": "old glass"}}
        })).unwrap();
        let (out, kept) = with_shipped(&current);
        assert_eq!(kept, vec!["MyOwn".to_string()]);
        assert!(out["Monazite"].get("MBA").is_none(), "MBA is now an alternative code of MB508");
        assert!(out["Monazite"].get("MB508").is_some());
        assert!(out["Glass"].get("BCR-2G").is_some() && out["Glass"].get("BCR2G").is_none());
        assert_eq!(out["Zircon"]["Plesovice"]["age"], 337.13);
    }

    #[test]
    fn laser_materials_join_the_epma_entries() {
        let mut nested: Map<String, Value> = serde_json::from_value(json!({
            "Monazite": {"WC425": {"name": "Grain 44069", "age": 425.0, "age_2s": 3.0, "source": "A"},
                         "MB508": {"name": "Moacyr", "age": 508.0, "age_2s": 15.0, "source": "G"}}
        }))
        .unwrap();
        let prefixes = vec![("M44".to_string(), "WC425".to_string()), ("MBA".to_string(), "MB508".to_string())];
        let builtin = chemchron_laser_core::refmat::builtin(&chemchron_laser_core::geochron::DecayConstants::default());
        merge_into_lab(&mut nested, &builtin, &prefixes, true);
        // 44069 (alias M44) and MBA joined the EPMA entries; glasses went to Glass
        assert_eq!(nested["Monazite"]["WC425"]["laser"]["name"], "44069");
        assert_eq!(nested["Monazite"]["MB508"]["laser"]["name"], "MBA");
        assert!(nested["Glass"].get("NIST612").is_some());
        assert_eq!(nested["Monazite"]["WC425"]["age"], 425.0, "EPMA fields kept");
        // and come back as the same laser list, the EPMA code added as a label alias
        let list = laser_list(&nested);
        let m = list.iter().find(|m| m.name == "44069").unwrap();
        assert!(m.matches_label("WC425") && m.matches_label("M44"));
        assert_eq!(list.len(), builtin.len());
        let orig = builtin.iter().find(|b| b.name == "NIST612").unwrap();
        assert_eq!(list.iter().find(|m| m.name == "NIST612").unwrap().values, orig.values);
        // removing a material from the laser list drops its laser part only
        let fewer: Vec<ReferenceMaterial> = list.into_iter().filter(|m| m.name != "44069" && m.name != "NIST610").collect();
        drop_missing(&mut nested, &fewer);
        assert!(nested["Monazite"]["WC425"].get("laser").is_none());
        assert_eq!(nested["Monazite"]["WC425"]["age"], 425.0);
        assert!(nested["Glass"].get("NIST610").is_none());
    }
}
