// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the lab settings live: the settings folder, an optional working
//! directory (named in a pointer file), and taking over the settings of
//! another settings folder.

use crate::row::*;
use crate::settings::{default_user_data_dir, Settings};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Files of the settings folder: (file, what it holds).
pub const SETTINGS_FILES: [(&str, &str); 13] = [
    ("reference_materials.json", "Reference materials of the lab: EPMA and laser values together"),
    ("long_term_reproducibility.json", "Chemical (EPMA) long-term log"),
    ("long_term_upb.json", "U–Pb long-term log"),
    ("long_term_uncertainty.json", "Chemical uncertainty model per mineral"),
    ("upb_settings.json", "U–Pb criteria, label prefixes, U–Pb uncertainty model"),
    ("decay_constants.json", "Decay constants (EPMA, Combine)"),
    ("display_settings.json", "Displayed decimal places"),
    ("flag_colors.json", "Sample group colours"),
    ("chondrite_normalization.json", "Chondrite normalisation sets"),
    ("weighted_mean_settings.json", "Outlier rejection"),
    ("mineral_id_thresholds.json", "Automatic mineral identification"),
    ("label_rules.json", "How laser labels are matched to EPMA labels"),
    ("recent_projects.json", "Recent projects"),
];

/// Uses `folder` as working directory from now on (settings in
/// `folder/option_data`); the current settings are
/// copied there when it has none yet. Returns the new settings folder.
pub fn set_working_directory(current: &Settings, folder: &Path) -> Result<PathBuf, String> {
    let target = folder.join("option_data");
    std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    if !target.join("reference_materials.json").exists() {
        let mut copy = current.clone();
        copy.dir = target.clone();
        copy.save()?;
    }
    let base = default_user_data_dir();
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let pointer = base.join("working_directory.json");
    std::fs::write(&pointer, serde_json::to_string_pretty(&json!({"working_dir": folder.to_string_lossy()})).unwrap())
        .map_err(|e| format!("{}: {e}", pointer.display()))?;
    Ok(target)
}

/// Takes over the settings of another settings folder (an `option_data`
/// folder): its reference materials are added, or replace the EPMA
/// fields of entries with the same code (laser values are kept); log entries
/// not logged here yet are added; the other settings are taken over.
pub fn import_from(s: &mut Settings, other: &Path) -> Result<String, String> {
    if !other.join("reference_materials.json").exists() && !other.join("decay_constants.json").exists() {
        return Err(format!("{} has no ChemChron settings files.", other.display()));
    }
    let o = Settings::load_from(other);
    let mut n_refs = 0;
    for (mineral, entries) in &o.refs_nested {
        let Value::Object(entries) = entries else { continue };
        let target = s.refs_nested.entry(mineral.clone()).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(target) = target else { continue };
        for (code, e) in entries {
            let mut new = e.clone();
            if let (Some(Value::Object(old)), Value::Object(n)) = (target.get(code), &mut new) {
                for k in ["laser", "aliases"] {
                    if let Some(l) = old.get(k) {
                        n.entry(k).or_insert_with(|| l.clone());
                    }
                }
            }
            target.insert(code.clone(), new);
            n_refs += 1;
        }
    }
    let key = |e: &Row| (text(e, "Row_ID"), text(e, "Label"), text(e, "Reference"), text(e, "Age_Ma"), text(e, "t68_Ma"), text(e, "Timestamp"));
    let mut n_log = 0;
    for (mine, theirs) in [(&mut s.long_term_log, &o.long_term_log), (&mut s.upb_log, &o.upb_log)] {
        let have: Vec<_> = mine.iter().map(key).collect();
        for e in theirs {
            if !have.contains(&key(e)) {
                mine.push(e.clone());
                n_log += 1;
            }
        }
    }
    s.decay = o.decay;
    s.decimals = o.decimals;
    s.uncertainty = o.uncertainty;
    s.upb = o.upb;
    s.flag_colors.extend(o.flag_colors);
    s.norm = o.norm;
    s.plot_export = o.plot_export;
    s.thresholds = o.thresholds;
    s.reject_outliers = o.reject_outliers;
    s.rejection_sigma = o.rejection_sigma;
    s.prefixes_to_aliases();
    s.save()?;
    Ok(format!("Took over {n_refs} reference materials, {n_log} log entries and the other settings from {}.", other.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_are_taken_over_and_laser_values_kept() {
        let base = std::env::temp_dir().join(format!("cc-workdir-{}", std::process::id()));
        let old = base.join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("reference_materials.json"), r#"{"Monazite": {"WC425": {"name": "44069", "age": 424.0, "age_2s": 2.0, "source": "lab"}, "TEST550": {"name": "test material", "age": 550.0, "age_2s": 15.0, "source": "test"}}}"#).unwrap();
        std::fs::write(old.join("long_term_reproducibility.json"), r#"{"entries": [{"Timestamp": "t", "Reference": "WC425", "Label": "a", "Age_Ma": 425.1, "Age_2s_Ma": 9.0, "SessionTag": "s1", "Mineral": "Monazite", "Row_ID": "x"}]}"#).unwrap();
        let mut s = Settings { dir: base.join("new"), ..Default::default() };
        if let Some(Value::Object(m)) = s.refs_nested.get_mut("Monazite") {
            if let Some(Value::Object(e)) = m.get_mut("WC425") {
                e.insert("laser".into(), json!({"name": "44069"}));
            }
        }
        let msg = import_from(&mut s, &old).unwrap();
        assert!(msg.contains("2 reference materials"), "{msg}");
        assert_eq!(s.refs_nested["Monazite"]["WC425"]["age"], 424.0);
        assert_eq!(s.refs_nested["Monazite"]["WC425"]["laser"]["name"], "44069");
        assert!(s.refs_nested["Monazite"].get("TEST550").is_some());
        assert_eq!(s.long_term_log.len(), 1);
        // a second import adds nothing twice
        import_from(&mut s, &old).unwrap();
        assert_eq!(s.long_term_log.len(), 1);
        assert!(base.join("new/reference_materials.json").exists());
    }
}
