// SPDX-License-Identifier: GPL-3.0-or-later
//! Lab settings shared by all projects (decay constants, reference
//! materials, uncertainty model, U-Pb criteria, long-term logs, ...), kept as
//! JSON files in one settings folder. Older file versions are converted on
//! loading.

use crate::constants::*;
use crate::ree::{merge_normalization_settings, NormSettings};
use crate::reference::Refs;
use crate::row::*;
use crate::uncertainty::MineralUncertainty;
use crate::upb::Criteria;
use crate::upbstats::UpbMineralUncertainty;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn default_user_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/ChemChron")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("ChemChron")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("ChemChron")
    }
}

/// The settings folder: a chosen working folder, else the default.
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CHEMCHRON_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let base = default_user_data_dir();
    let pointer = base.join("working_directory.json");
    if let Ok(t) = std::fs::read_to_string(&pointer) {
        if let Ok(v) = serde_json::from_str::<Value>(&t) {
            if let Some(d) = v.get("working_dir").and_then(|x| x.as_str()) {
                if Path::new(d).is_dir() {
                    return Path::new(d).join("option_data");
                }
            }
        }
    }
    base.join("option_data")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpbSettings {
    pub disc_definition: String,
    pub disc_threshold_pct: f64,
    pub disc_ellipse_test: bool,
    pub disc_hard_limit_pct: f64,
    pub precision_filter: bool,
    pub max_rel_2se_pct: f64,
    pub systematic_pct: f64,
    pub systematic_source: String,
    pub uncertainty_by_mineral: BTreeMap<String, UpbMineralUncertainty>,
    pub reference_prefixes: BTreeMap<String, String>,
    pub summary_age_type: String,
    /// Add the decay-constant uncertainties to the systematic term (U–Pb and chemical ages).
    pub decay_in_systematic: bool,
    /// Version of the uncertainty-model settings (3: systematic term from the long-term log, excess from the reference materials).
    /// Missing in older files (read as 0, so they are brought up to date once).
    #[serde(default)]
    pub model_version: u32,
}

impl Default for UpbSettings {
    fn default() -> Self {
        UpbSettings {
            disc_definition: "76".into(),
            disc_threshold_pct: crate::upb::DEFAULT_DISC_THRESHOLD_PCT,
            disc_ellipse_test: true,
            disc_hard_limit_pct: crate::upb::DEFAULT_DISC_HARD_LIMIT_PCT,
            precision_filter: true,
            max_rel_2se_pct: crate::upb::DEFAULT_MAX_REL_2SE_PCT,
            systematic_pct: crate::upb::DEFAULT_SYSTEMATIC_PCT,
            systematic_source: "log".into(),
            uncertainty_by_mineral: BTreeMap::new(),
            reference_prefixes: [("MBA", "MB508"), ("M44", "WC425")]
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            summary_age_type: "t68".into(),
            decay_in_systematic: false,
            model_version: 3,
        }
    }
}

impl UpbSettings {
    pub fn criteria(&self) -> Criteria {
        Criteria {
            definition: self.disc_definition.clone(),
            threshold_pct: self.disc_threshold_pct,
            ellipse_test: self.disc_ellipse_test,
            hard_limit_pct: Some(self.disc_hard_limit_pct),
            max_rel_2se_pct: if self.precision_filter { Some(self.max_rel_2se_pct) } else { None },
        }
    }
    /// Per-mineral settings with the old global systematic value as fallback.
    pub fn by_mineral(&self) -> BTreeMap<String, UpbMineralUncertainty> {
        MINERAL_NAMES
            .iter()
            .map(|m| {
                let mut s = self.uncertainty_by_mineral.get(*m).cloned().unwrap_or_else(|| UpbMineralUncertainty {
                    systematic_pct: self.systematic_pct,
                    systematic_source: self.systematic_source.clone(),
                    ..Default::default()
                });
                for r in crate::upbstats::RATIOS {
                    s.excess_manual_pct.entry(r.to_string()).or_insert(0.0);
                }
                (m.to_string(), s)
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlotExport {
    pub width_mm: f64,
    pub height_mm: f64,
    pub aspect: String,
    pub font_pt: f64,
    pub format: String,
    pub dpi: u32,
    pub include_title: bool,
}

impl Default for PlotExport {
    fn default() -> Self {
        PlotExport { width_mm: 85.0, height_mm: 64.0, aspect: "4 : 3".into(), font_pt: 7.0, format: "svg".into(), dpi: 600, include_title: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub dir: PathBuf,
    pub decay: DecayConstants,
    pub decimals: DecimalPlaces,
    /// mineral -> code -> {name, age, age_2s, source, age_68, ...}
    pub refs_nested: Map<String, Value>,
    pub uncertainty: BTreeMap<String, MineralUncertainty>,
    pub upb: UpbSettings,
    pub flag_colors: BTreeMap<String, String>,
    pub norm: NormSettings,
    pub reject_outliers: bool,
    pub rejection_sigma: f64,
    pub rank_by_age: bool,
    pub show_actual_below_dl: bool,
    pub show_uncertainty_columns: bool,
    pub recent_projects: Vec<String>,
    pub thresholds: MineralIdThresholds,
    pub plot_export: PlotExport,
    /// How laser labels are matched to EPMA labels.
    pub label_rules: crate::trace::LabelRules,
    pub export_columns: BTreeMap<String, BTreeMap<String, bool>>,
    pub long_term_log: Vec<Row>,
    pub upb_log: Vec<Row>,
    /// Dark window colours.
    pub dark: bool,
    /// Keep the laser session (selections, corrections) inside the project file.
    pub laser_in_project: bool,
}

pub const LONG_TERM_FIELDS: [&str; 8] = ["Timestamp", "Reference", "Label", "Age_Ma", "Age_2s_Ma", "SessionTag", "Mineral", "Row_ID"];

pub fn upb_log_fields() -> Vec<String> {
    let mut v: Vec<String> = ["Timestamp", "Reference", "Label", "Mineral", "SessionTag", "Row_ID"].map(String::from).to_vec();
    for t in ["t68", "t75", "t76", "t82"] {
        for s in ["Ma", "2s_Ma", "2s_point_Ma"] {
            v.push(format!("{t}_{s}"));
        }
    }
    v.push("Disc76_pct".into());
    v.push("Disc75_pct".into());
    v
}

fn read_json(path: &Path) -> Option<Value> {
    let t = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&t).ok()
}

/// The lab's reference list as shipped with the program (a new installation starts with it).
pub const SHIPPED_REFERENCE_MATERIALS: &str = include_str!("../../assets/reference_materials.json");

fn default_refs() -> Map<String, Value> {
    let mut m = match serde_json::from_str::<Value>(SHIPPED_REFERENCE_MATERIALS) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    for mineral in MINERAL_NAMES {
        m.entry(mineral.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    m
}

impl Default for Settings {
    fn default() -> Self {
        let mut unc = BTreeMap::new();
        for m in MINERAL_NAMES {
            unc.insert(m.to_string(), MineralUncertainty { manual_override_pct: DEFAULT_LONG_TERM_REL_PCT, ..Default::default() });
        }
        Settings {
            dir: config_dir(),
            decay: DecayConstants::default(),
            decimals: DecimalPlaces::default(),
            refs_nested: default_refs(),
            uncertainty: unc,
            upb: UpbSettings::default(),
            flag_colors: BTreeMap::new(),
            norm: NormSettings::default(),
            reject_outliers: false,
            rejection_sigma: crate::stats::DEFAULT_OUTLIER_REJECTION_SIGMA,
            rank_by_age: false,
            show_actual_below_dl: false,
            show_uncertainty_columns: false,
            recent_projects: Vec::new(),
            thresholds: MineralIdThresholds::default(),
            plot_export: PlotExport::default(),
            label_rules: Default::default(),
            export_columns: BTreeMap::new(),
            long_term_log: Vec::new(),
            upb_log: Vec::new(),
            dark: true,
            laser_in_project: true,
        }
    }
}

impl Settings {
    /// Settings from `dir` (missing files keep their defaults).
    pub fn load_from(dir: &Path) -> Settings {
        let mut s = Settings { dir: dir.to_path_buf(), ..Default::default() };
        let f = |n: &str| dir.join(n);
        if let Some(v) = read_json(&f("decay_constants.json")) {
            if let Ok(mut d) = serde_json::from_value::<DecayConstants>(v) {
                d.sync_fractions();
                s.decay = d;
            }
        }
        if let Some(v) = read_json(&f("display_settings.json")) {
            if let Ok(d) = serde_json::from_value::<DecimalPlaces>(v) {
                s.decimals = d;
            }
        }
        if let Some(Value::Object(o)) = read_json(&f("reference_materials.json")) {
            let flat_schema = o.values().any(|v| v.get("age").is_some() && v.get("age_2s").is_some());
            let mut nested = if flat_schema {
                let mut m = Map::new();
                m.insert("Monazite".into(), Value::Object(o));
                m
            } else {
                o
            };
            for m in MINERAL_NAMES {
                nested.entry(m.to_string()).or_insert_with(|| Value::Object(Map::new()));
            }
            s.refs_nested = nested;
        }
        if let Some(Value::Object(o)) = read_json(&f("long_term_uncertainty.json")) {
            let legacy = o.get("manual_override_pct").and_then(|v| v.as_f64());
            for m in MINERAL_NAMES {
                let mut entry = MineralUncertainty { manual_override_pct: DEFAULT_LONG_TERM_REL_PCT, ..Default::default() };
                if let Some(p) = legacy {
                    entry.manual_override_pct = p;
                } else if let Some(v) = o.get(m) {
                    if let Ok(e) = serde_json::from_value::<MineralUncertainty>(v.clone()) {
                        entry = e;
                    }
                }
                s.uncertainty.insert(m.to_string(), entry);
            }
        }
        if let Some(v) = read_json(&f("upb_settings.json")) {
            if let Ok(u) = serde_json::from_value::<UpbSettings>(v) {
                s.upb = u;
                if !matches!(s.upb.disc_definition.as_str(), "76" | "75") {
                    s.upb.disc_definition = "76".into();
                }
                if !["t68", "t75", "t76", "t82"].contains(&s.upb.summary_age_type.as_str()) {
                    s.upb.summary_age_type = "t68".into();
                }
                // Settings from before the common uncertainty model: the systematic
                // term now comes from the long-term log (manual value until it
                // holds enough sessions), for U–Pb and chemical ages alike.
                if s.upb.model_version < 2 {
                    s.upb.systematic_source = "log".into();
                    for m in s.upb.uncertainty_by_mineral.values_mut() {
                        m.systematic_source = "log".into();
                    }
                    for m in s.uncertainty.values_mut() {
                        m.systematic_source = "log".into();
                    }
                }
                // ... and the U–Pb random excess comes from the reference
                // materials of each session (the laser part follows it too)
                if s.upb.model_version < 3 {
                    for m in s.upb.uncertainty_by_mineral.values_mut() {
                        m.excess_source = "project".into();
                    }
                }
                s.upb.model_version = 3;
            }
        }
        if let Some(Value::Object(o)) = read_json(&f("flag_colors.json")) {
            s.flag_colors = o.into_iter().filter_map(|(k, v)| v.as_str().map(|c| (k, c.to_string()))).collect();
        }
        s.norm = merge_normalization_settings(read_json(&f("chondrite_normalization.json")).as_ref());
        if let Some(v) = read_json(&f("weighted_mean_settings.json")) {
            s.reject_outliers = v.get("reject_outliers").and_then(|x| x.as_bool()).unwrap_or(false);
            s.rejection_sigma = v.get("rejection_sigma").and_then(|x| x.as_f64()).unwrap_or(2.0);
            s.rank_by_age = v.get("rank_by_age").and_then(|x| x.as_bool()).unwrap_or(false);
        }
        if let Some(v) = read_json(&f("detection_limit_display.json")) {
            s.show_actual_below_dl = v.get("show_actual_values").and_then(|x| x.as_bool()).unwrap_or(false);
            s.show_uncertainty_columns = v.get("show_uncertainty_columns").and_then(|x| x.as_bool()).unwrap_or(false);
        }
        if let Some(v) = read_json(&f("recent_projects.json")) {
            s.recent_projects = v.get("paths").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        }
        if let Some(v) = read_json(&f("mineral_id_thresholds.json")) {
            if let Ok(t) = serde_json::from_value(v) {
                s.thresholds = t;
            }
        }
        if let Some(v) = read_json(&f("label_rules.json")) {
            if let Ok(r) = serde_json::from_value(v) {
                s.label_rules = r;
            }
        }
        if let Some(v) = read_json(&f("plot_export.json")) {
            if let Ok(p) = serde_json::from_value(v) {
                s.plot_export = p;
            }
        }
        if let Some(v) = read_json(&f("export_columns.json")) {
            if let Ok(p) = serde_json::from_value(v) {
                s.export_columns = p;
            }
        }
        if let Some(v) = read_json(&f("chemchron2_display.json")) {
            s.dark = v.get("dark").and_then(|x| x.as_bool()).unwrap_or(true);
            s.laser_in_project = v.get("laser_in_project").and_then(|x| x.as_bool()).unwrap_or(true);
        }
        if let Some(v) = read_json(&f("long_term_reproducibility.json")) {
            if let Some(a) = v.get("entries").and_then(|e| e.as_array()) {
                for e in a {
                    let Some(o) = e.as_object() else { continue };
                    let mut r = o.clone();
                    let Some(age) = num_loose(&r, "Age_Ma") else { continue };
                    set(&mut r, "Age_Ma", jnum(age));
                    s.long_term_log.push(r);
                }
            }
        }
        if let Some(v) = read_json(&f("long_term_upb.json")) {
            if let Some(a) = v.get("entries").and_then(|e| e.as_array()) {
                let fields = upb_log_fields();
                for e in a {
                    let Some(o) = e.as_object() else { continue };
                    let mut r = Row::new();
                    for k in &fields {
                        let v = o.get(k).cloned().unwrap_or_else(blank);
                        let numeric = k.ends_with("_Ma") || k.starts_with("Disc");
                        if numeric {
                            let x = num_loose(o, k);
                            set(&mut r, k, jopt(x));
                        } else {
                            set(&mut r, k, v);
                        }
                    }
                    s.upb_log.push(r);
                }
            }
        }
        s.prefixes_to_aliases();
        s
    }

    /// Label prefixes of older settings files (`M44` for `WC425`)
    /// become alternative codes of their reference material.
    pub fn prefixes_to_aliases(&mut self) {
        let prefixes: Vec<(String, String)> = self.upb.reference_prefixes.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
        for (p, code) in prefixes {
            let mut done = false;
            for (_, entries) in self.refs_nested.iter_mut() {
                if let Some(Value::Object(e)) = entries.as_object_mut().and_then(|m| m.get_mut(&code)) {
                    let list = e.entry("aliases").or_insert_with(|| json!([]));
                    if !list.is_array() {
                        *list = json!([]);
                    }
                    let a = list.as_array_mut().unwrap();
                    if !a.iter().any(|x| x.as_str().is_some_and(|x| x.eq_ignore_ascii_case(&p))) {
                        a.push(Value::String(p.clone()));
                    }
                    done = true;
                    break;
                }
            }
            if done {
                self.upb.reference_prefixes.remove(&p);
            }
        }
    }

    /// Every code that identifies a reference material in a label, with the
    /// material's code: the code itself, its alternative codes and the laser
    /// name (e.g. `WC425`, `M44`, `44069` for `WC425`).
    pub fn label_codes(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for (_, entries) in &self.refs_nested {
            let Some(entries) = entries.as_object() else { continue };
            for (code, e) in entries {
                for a in crate::reference::codes_of(code, e) {
                    if !out.iter().any(|(x, _)| x.eq_ignore_ascii_case(&a)) {
                        out.push((a, code.clone()));
                    }
                }
            }
        }
        for (p, c) in &self.upb.reference_prefixes {
            out.push((p.clone(), c.clone()));
        }
        out
    }

    pub fn load() -> Settings {
        Settings::load_from(&config_dir())
    }

    fn write(&self, name: &str, v: &Value) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let p = self.dir.join(name);
        std::fs::write(&p, serde_json::to_string_pretty(v).unwrap()).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn save(&self) -> Result<(), String> {
        self.write("decay_constants.json", &serde_json::to_value(&self.decay).unwrap())?;
        self.write("display_settings.json", &serde_json::to_value(self.decimals).unwrap())?;
        self.write("reference_materials.json", &Value::Object(self.refs_nested.clone()))?;
        self.write("long_term_uncertainty.json", &serde_json::to_value(&self.uncertainty).unwrap())?;
        self.write("upb_settings.json", &serde_json::to_value(&self.upb).unwrap())?;
        self.write("flag_colors.json", &serde_json::to_value(&self.flag_colors).unwrap())?;
        self.write("chondrite_normalization.json", &serde_json::to_value(&self.norm).unwrap())?;
        self.write(
            "weighted_mean_settings.json",
            &json!({"reject_outliers": self.reject_outliers, "rejection_sigma": self.rejection_sigma, "rank_by_age": self.rank_by_age}),
        )?;
        self.write(
            "detection_limit_display.json",
            &json!({"show_actual_values": self.show_actual_below_dl, "show_uncertainty_columns": self.show_uncertainty_columns}),
        )?;
        let recent: Vec<&String> = self.recent_projects.iter().take(10).collect();
        self.write("recent_projects.json", &json!({ "paths": recent }))?;
        self.write("mineral_id_thresholds.json", &serde_json::to_value(&self.thresholds).unwrap())?;
        self.write("plot_export.json", &serde_json::to_value(&self.plot_export).unwrap())?;
        self.write("label_rules.json", &serde_json::to_value(&self.label_rules).unwrap())?;
        self.write("export_columns.json", &serde_json::to_value(&self.export_columns).unwrap())?;
        self.write("chemchron2_display.json", &json!({ "dark": self.dark, "laser_in_project": self.laser_in_project }))?;
        self.save_logs()
    }

    pub fn save_logs(&self) -> Result<(), String> {
        let lt: Vec<Value> = self
            .long_term_log
            .iter()
            .map(|e| Value::Object(LONG_TERM_FIELDS.iter().map(|k| (k.to_string(), e.get(*k).cloned().unwrap_or_else(blank))).collect()))
            .collect();
        self.write("long_term_reproducibility.json", &json!({ "entries": lt }))?;
        let f = upb_log_fields();
        let up: Vec<Value> =
            self.upb_log.iter().map(|e| Value::Object(f.iter().map(|k| (k.clone(), e.get(k).cloned().unwrap_or_else(blank))).collect())).collect();
        self.write("long_term_upb.json", &json!({ "entries": up }))
    }

    /// All reference materials by code (later minerals win on duplicates).
    pub fn refs(&self) -> Refs {
        let mut flat = Refs::new();
        for m in MINERAL_NAMES {
            if let Some(Value::Object(o)) = self.refs_nested.get(m) {
                for (k, v) in o {
                    let mut entry = v.clone();
                    if let Value::Object(e) = &mut entry {
                        e.entry("mineral").or_insert_with(|| Value::String(m.to_string()));
                    }
                    flat.insert(k.clone(), entry);
                }
            }
        }
        flat
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_string());
        self.recent_projects.truncate(10);
    }
}

// ---- project files --------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct EpmaProject {
    pub rows: Vec<Row>,
    pub active_oxides: Vec<String>,
    pub meta: Map<String, Value>,
}

/// Reads a project file (`{"version", "active_oxides", "rows"}`, older file
/// versions are converted on loading).
pub fn project_from_value(v: &Value, name: &str) -> Result<EpmaProject, String> {
    let o = v.as_object().ok_or_else(|| format!("{name} does not look like a ChemChron project file."))?;
    let o = match o.get("epma").and_then(|e| e.as_object()) {
        Some(e) if !o.contains_key("rows") => e,
        _ => o,
    };
    let rows = o.get("rows").and_then(|r| r.as_array()).ok_or_else(|| format!("{name} does not look like a ChemChron project file."))?;
    let mut rows: Vec<Row> = rows.iter().filter_map(|r| r.as_object().cloned()).collect();
    crate::reference::normalize_within_values(&mut rows);
    let active = o
        .get("active_oxides")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let meta = o.iter().filter(|(k, _)| k.as_str() != "rows" && k.as_str() != "active_oxides").map(|(k, v)| (k.clone(), v.clone())).collect();
    Ok(EpmaProject { rows, active_oxides: active, meta })
}

pub fn project_to_value(p: &EpmaProject) -> Value {
    json!({
        "version": 1,
        "active_oxides": p.active_oxides,
        "rows": p.rows,
        "detection_rule": crate::sessions::DETECTION_RULE_VERSION,
    })
}

pub fn load_project(path: &Path) -> Result<EpmaProject, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
    project_from_value(&v, &path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
}

pub fn save_project(path: &Path, p: &EpmaProject) -> Result<(), String> {
    std::fs::write(path, serde_json::to_string_pretty(&project_to_value(p)).unwrap()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Indices of chemical log entries logged from one row.
pub fn find_log_entries_for_row(entries: &[Row], row_id: &str, label: &str, reference: &str, age: Option<f64>) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let id = text(e, "Row_ID");
        if !id.is_empty() {
            if !row_id.is_empty() && id == row_id {
                out.push(i);
            }
            continue;
        }
        if !label.is_empty() && text(e, "Label") == label && text(e, "Reference") == reference {
            if let (Some(a), Some(b)) = (age, num(e, "Age_Ma")) {
                if (a - b).abs() <= 1e-6 {
                    out.push(i);
                }
            }
        }
    }
    out
}
