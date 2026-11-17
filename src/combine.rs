// SPDX-License-Identifier: GPL-3.0-or-later
//! Combine part: link laser U–Pb and trace-element data to EPMA analyses by
//! label (from the Laser part or from files), the linked table and the
//! Summary. Plots are in `combine_plots`, long-term logs in `longterm`,
//! exports in `export`.

use crate::app::{frame_part, nav, ChemApp};
use crate::epma::{table_page, GroupPick};
use crate::rowtable::{self, Look, TableState};
use chemchron_core::columns;
use chemchron_core::constants::*;
use chemchron_core::row::*;
use chemchron_core::trace::{TraceImport, TraceRecord};
use chemchron_core::upb::Record;
use egui::{RichText, Ui};
use std::path::PathBuf;
use chemchron_laser::plot::View;
use chemchron_laser::theme::{section, segmented, stat};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Data,
    Summary,
    Isochron,
    WeightedMean,
    Concordia,
    UpbMean,
    Ree,
    Composition,
    Compare,
    AgeDist,
    LongTerm,
    Export,
}

impl Page {
    pub fn from_name(n: &str) -> Option<Page> {
        Some(match n.to_lowercase().as_str() {
            "data" | "link" | "analyses" | "table" => Page::Data,
            "summary" => Page::Summary,
            "isochron" => Page::Isochron,
            "weighted" | "weightedmean" | "mean" => Page::WeightedMean,
            "concordia" => Page::Concordia,
            "upbmean" | "isotopic" => Page::UpbMean,
            "ree" => Page::Ree,
            "composition" => Page::Composition,
            "compare" | "ages" => Page::Compare,
            "distribution" | "kde" | "agedist" => Page::AgeDist,
            "longterm" | "long-term" => Page::LongTerm,
            "export" => Page::Export,
            _ => return None,
        })
    }
}


#[derive(Default)]
pub struct FileTable {
    pub path: Option<PathBuf>,
    pub sheets: Vec<String>,
    pub sheet: String,
    pub table: Vec<Vec<String>>,
    pub header_row: usize,
    pub label_col: usize,
    pub group_col: Option<usize>,
    pub unit: String,
    pub upb: bool,
}

#[derive(Default)]
pub struct CombineState {
    pub page: Page,
    pub file: FileTable,
    pub pending: Option<PendingImport>,
    pub add_unmatched: bool,
    pub new_mineral: String,
    pub session: String,
    pub include_laser_refs: bool,
    pub show_refs: bool,
    pub table: TableState,
    pub ref_table: TableState,
    pub comp_table: TableState,
    pub comp_ref_table: TableState,
    pub data_tab: DataTab,
    /// Weighted mean page shows U–Pb ages (else chemical ages).
    pub wm_upb: bool,
    pub sum_refs: TableState,
    pub sum_flags: TableState,
    pub sum_comp_refs: TableState,
    pub sum_comp_flags: TableState,
    pub summary_view: SummaryView,
    pub summary_sd: bool,
    pub conc_group: GroupPick,
    pub conc_wetherill: bool,
    pub conc_age: bool,
    pub conc_decay: bool,
    pub conc_discordia: bool,
    pub conc_anchor: bool,
    pub conc_anchor_ma: f64,
    pub conc_show_disc: bool,
    pub conc_view: View,
    pub iwm_group: GroupPick,
    pub iwm_type: String,
    pub iwm_view: View,
    pub iwm_show_disc: bool,
    pub ree_groups: bool,
    /// Shade min to max instead of the interquartile range.
    pub ree_range: bool,
    pub ree_y: bool,
    pub ree_src: chemchron_core::ree::Source,
    pub ree_sel: Vec<GroupPick>,
    pub ree_view: View,
    pub comp_kind: usize,
    pub comp_sel: Vec<GroupPick>,
    pub comp_x: String,
    pub comp_y: String,
    pub comp_z: String,
    pub comp_log: bool,
    pub comp_view: View,
    pub cmp_type: String,
    pub cmp_view: View,
    pub kde_view: View,
    pub lt_view: View,
    pub dist_sel: Vec<GroupPick>,
    /// 0 chemical, 1 U–Pb, 2 both.
    pub dist_kind: u8,
    pub dist_type: String,
    pub dist_pool: bool,
    pub dist_bw: f64,
    pub dist_view: View,
    pub kde_bw: f64,
    pub lt_upb: bool,
    pub lt_tag: String,
    pub lt_type: String,
    pub lt_mineral: String,
    /// Filters of the long-term log entries (Settings > Uncertainty models).
    pub lt_filter: LogFilter,
    /// Label typed to try the label rules (Settings > U–Pb & Combine).
    pub label_try: String,
    pub upb_mineral: String,
    pub export_choice: std::collections::BTreeMap<String, bool>,
    pub init: bool,
}

/// Which long-term log entries are listed (empty: all).
#[derive(Default, Clone)]
pub struct LogFilter {
    pub mineral: String,
    pub reference: String,
    pub session: String,
    pub search: String,
}

/// An import waiting for confirmation.
pub struct PendingImport {
    pub upb: Option<(Vec<Record>, Vec<String>)>,
    pub trace: Option<TraceImport>,
    pub ignored: Vec<String>,
    pub source: String,
    pub from_laser: bool,
}

/// Starting choices of the Combined results pages.
pub fn init_defaults(c: &mut CombineState) {
    c.init = true;
    c.conc_age = true;
    c.conc_show_disc = true;
    c.iwm_type = "t68".into();
    c.cmp_type = "t68".into();
    c.dist_type = "t68".into();
    c.lt_type = "t68".into();
    c.lt_mineral = DEFAULT_MINERAL.into();
    c.upb_mineral = DEFAULT_MINERAL.into();
    c.new_mineral = DEFAULT_MINERAL.into();
    c.ree_y = true;
    c.ree_groups = false;
    c.include_laser_refs = true;
    c.add_unmatched = true;
    c.comp_x = "ThO2_wt%".into();
    c.comp_y = "UO2_wt%".into();
    c.comp_z = "PbO_wt%".into();
    c.iwm_show_disc = true;
}

pub fn show(app: &mut ChemApp, ui: &mut Ui) {
    if !app.combine.init {
        init_defaults(&mut app.combine);
    }
    frame_part(app, ui, "combine", nav_ui, |app, ui| match app.combine.page {
        Page::Data => page_data(app, ui),
        Page::Summary => page_summary(app, ui),
        Page::Isochron => crate::epma_pages::page_isochron(app, ui),
        Page::WeightedMean => page_weighted_mean(app, ui),
        Page::Concordia => crate::combine_plots::page_concordia(app, ui),
        Page::UpbMean => {
            // the U–Pb tab of the Weighted mean page
            app.combine.wm_upb = true;
            app.combine.page = Page::WeightedMean;
            page_weighted_mean(app, ui)
        }
        Page::Ree => crate::combine_plots::page_ree(app, ui),
        Page::Composition => crate::combine_plots::page_composition(app, ui),
        Page::Compare => crate::combine_plots::page_compare(app, ui),
        Page::AgeDist => crate::combine_plots::page_age_distribution(app, ui),
        Page::LongTerm => crate::longterm::page_long_term(app, ui),
        Page::Export => crate::export::page_export(app, ui),
    });
}

fn nav_ui(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let upb = app.data.rows.iter().any(chemchron_core::upb::has_upb);
    let any = !app.data.rows.is_empty();
    let chem = app.data.rows.iter().any(|r| !truthy(r, "No_EPMA"));
    let dot = |b: bool| Some(if b { t.ok } else { t.border });
    let pages = [
        (Page::Data, "Data", dot(any), true),
        (Page::Summary, "Summary", dot(any), true),
        (Page::Isochron, "Chemical isochron", dot(chem), true),
        (Page::WeightedMean, "Weighted mean", dot(chem || upb), true),
        (Page::Concordia, "Concordia", dot(upb), true),
        (Page::Ree, "REE diagram", dot(any), true),
        (Page::Composition, "Composition plots", dot(any), true),
        (Page::Compare, "Chemical vs U–Pb", dot(upb && any), true),
        (Page::AgeDist, "Age distribution", dot(chem || upb), true),
        (Page::LongTerm, "Long-term logs", dot(!app.settings.long_term_log.is_empty() || !app.settings.upb_log.is_empty()), true),
        (Page::Export, "Export", None, false),
    ];
    nav(ui, &t, &mut app.combine.page, &pages);
    ui.add_space(12.0);
    ui.label(RichText::new("All results together, linked by analysis label: chemical ages, U–Pb ages and trace elements, each also on its own. Figures for papers are saved from here.").color(t.dim).size(11.5));
}

// ---- linking -----------------------------------------------------------------------------

fn load_file(app: &mut ChemApp, path: PathBuf, upb: bool) {
    let sheets = chemchron_core::table::list_sheets(&path).unwrap_or_default();
    let sheet = sheets.first().cloned().unwrap_or_default();
    match chemchron_core::table::read_table(&path, if sheet.is_empty() { None } else { Some(&sheet) }) {
        Ok(table) => {
            let f = &mut app.combine.file;
            f.upb = upb;
            f.sheets = sheets;
            f.sheet = sheet;
            f.header_row = if upb { detect_upb_header(&table) } else { chemchron_core::trace::detect_header_row(&table) };
            let header = table.get(f.header_row).cloned().unwrap_or_default();
            f.label_col = chemchron_core::trace::guess_label_column(&header);
            f.group_col = if upb { chemchron_core::upb::guess_group_column(&header, f.label_col) } else { None };
            f.unit = "ppm".into();
            f.table = table;
            f.path = Some(path);
            app.combine.pending = None;
        }
        Err(e) => app.say(e, true),
    }
}

fn detect_upb_header(table: &[Vec<String>]) -> usize {
    for (i, r) in table.iter().take(50).enumerate() {
        if r.iter().filter(|c| chemchron_core::upb::parse_upb_header(c).is_some()).count() >= 2 {
            return i;
        }
    }
    0
}

fn plan_file(app: &mut ChemApp) {
    let f = &app.combine.file;
    let header = f.table.get(f.header_row).cloned().unwrap_or_default();
    let source = f.path.as_ref().map(|p| chemchron_core::pipeline::file_name(p)).unwrap_or_default();
    if f.upb {
        let (cols, ignored) = chemchron_core::upb::plan_upb_columns(&header, f.label_col);
        let (recs, dups) = chemchron_core::upb::build_upb_records(&f.table, f.header_row, f.label_col, &cols, f.group_col);
        app.combine.pending = Some(PendingImport { upb: Some((recs, dups)), trace: None, ignored, source, from_laser: false });
    } else {
        let (cols, ignored) = chemchron_core::trace::plan_columns(&header, f.label_col);
        let imp = chemchron_core::trace::build_records(&f.table, f.header_row, f.label_col, &cols, &f.unit);
        app.combine.pending = Some(PendingImport { upb: None, trace: Some(imp), ignored, source, from_laser: false });
    }
    if app.combine.session.is_empty() {
        app.combine.session = chemchron_core::sessions::session_name_from_path(&app.combine.pending.as_ref().unwrap().source);
    }
}

fn plan_laser(app: &mut ChemApp) {
    let spots = crate::link::laser_spots(&app.laser, app.combine.include_laser_refs);
    let upb = if spots.upb.is_empty() { None } else { Some((spots.upb, Vec::new())) };
    let trace = if spots.trace.records.is_empty() { None } else { Some(spots.trace) };
    if upb.is_none() && trace.is_none() {
        app.say("The laser session has no reduced U–Pb ratios or trace elements yet. Set up the corrections in the Laser part first.", true);
        return;
    }
    app.combine.session = spots.session_name.clone();
    app.combine.pending = Some(PendingImport { upb, trace, ignored: Vec::new(), source: format!("Laser part: {}", spots.session_name), from_laser: true });
}

/// Reads the laser results and links them at once (command line `--link`).
pub fn link_laser_now(app: &mut ChemApp) {
    app.combine.include_laser_refs = true;
    app.combine.add_unmatched = true;
    app.combine.new_mineral = DEFAULT_MINERAL.into();
    plan_laser(app);
    apply_pending(app);
    apply_laser_minerals(app);
}

/// Rows that came only from the laser get the mineral of their laser group
/// (LA-ICP-MS > Signal & selections > group > Mineral).
pub fn apply_laser_minerals(app: &mut ChemApp) {
    let s = &app.laser.session;
    let by_label: std::collections::HashMap<String, String> = s
        .selections
        .iter()
        .map(|sel| (chemchron_core::trace::normalize_label(&crate::link::spot_label(&sel.label)), s.mineral_of(sel.group)))
        .collect();
    let mut changed = false;
    for r in app.data.rows.iter_mut() {
        if !truthy(r, "No_EPMA") {
            continue;
        }
        if let Some(m) = by_label.get(&chemchron_core::trace::normalize_label(&text(r, "Label"))) {
            if text(r, "Mineral") != *m {
                set(r, "Mineral", jstr(m));
                changed = true;
            }
        }
    }
    if changed {
        app.data.refresh(&app.settings);
    }
}

/// The reference material of a spot: for laser results the laser group says
/// it (a sample group is never a reference, whatever its label); otherwise
/// the label's code or alternative code.
fn reference_of_record(app: &ChemApp, rec: &Record, prefixes: &[(String, String)]) -> Option<String> {
    match text(&rec.values, "UPb_Laser_Kind").as_str() {
        "sample" => None,
        "reference" => crate::refsync::lab_code_of_laser_rm(&app.settings.refs_nested, &text(&rec.values, "UPb_Laser_RM")).or_else(|| chemchron_core::upb::reference_for_label(&rec.label, prefixes)),
        _ => chemchron_core::upb::reference_for_label(&rec.label, prefixes),
    }
}

/// Applies the pending import to the analysis table.
fn apply_pending(app: &mut ChemApp) {
    let Some(p) = app.combine.pending.take() else { return };
    let prefixes: Vec<(String, String)> = app.settings.label_codes();
    let refs = app.settings.refs();
    let decay = app.settings.decay.clone();
    let dp = app.settings.decimals.age;
    let session = if app.combine.session.trim().is_empty() { chemchron_core::sessions::session_name_from_path(&p.source) } else { app.combine.session.trim().to_string() };
    app.data.checkpoint(&format!("Link data from {}", p.source));
    let mut summary = Vec::new();
    let mut conflicts = Vec::new();
    let add = app.combine.add_unmatched;
    let mineral_default = app.combine.new_mineral.clone();
    let mut n_new = 0;
    if let Some((recs, _)) = &p.upb {
        let labels: Vec<String> = recs.iter().map(|r| r.label.clone()).collect();
        let (matched, unmatched) = chemchron_core::trace::match_labels(&app.data.rows, &labels, &app.settings.label_rules);
        let mut grouped = 0;
        for (i, ri) in &matched {
            let rec = &recs[*ri];
            let row = app.data.rows[*i].clone();
            let mut up = chemchron_core::upb::upb_update_for_row(&row, rec, &p.source, Some(&session));
            let mut merged = row.clone();
            for (k, v) in &up {
                merged.insert(k.clone(), v.clone());
            }
            for (k, v) in chemchron_core::upb::compute_upb_fields(&merged, &decay, dp) {
                up.insert(k, v);
            }
            if !rec.group.is_empty() && !truthy(&row, "Is_Reference") {
                let cur = text(&row, "Flag");
                if cur.is_empty() {
                    set(&mut up, "Flag", jstr(&rec.group));
                    grouped += 1;
                } else if cur != rec.group {
                    conflicts.push(format!("{}: {} (file: {})", text(&row, "Label"), cur, rec.group));
                }
            }
            // a laser-only row follows its laser group (sample or which reference)
            if truthy(&row, "No_EPMA") && !text(&rec.values, "UPb_Laser_Kind").is_empty() {
                let code = reference_of_record(app, rec, &prefixes);
                set(&mut up, "Is_Reference", serde_json::Value::Bool(code.is_some()));
                set(&mut up, "Reference", jstr(code.clone().unwrap_or_default()));
                if code.is_some() {
                    set(&mut up, "Flag", jstr(""));
                } else if text(&row, "Flag").is_empty() || truthy(&row, "Is_Reference") {
                    set(&mut up, "Flag", jstr(&rec.group));
                }
            }
            for (k, v) in up {
                app.data.rows[*i].insert(k, v);
            }
        }
        if add {
            for ri in &unmatched {
                let rec = &recs[*ri];
                let code = reference_of_record(app, rec, &prefixes);
                let mut mineral = mineral_default.clone();
                if let Some(c) = &code {
                    if let Some(m) = refs.get(c).and_then(|r| r.get("mineral")).and_then(|m| m.as_str()) {
                        mineral = m.to_string();
                    }
                }
                let mut base = Row::new();
                set(&mut base, "Label", jstr(crate::link::spot_label(&rec.label)));
                set(&mut base, "Row_ID", jstr(chemchron_core::processing::new_row_id()));
                set(&mut base, "Is_Reference", serde_json::Value::Bool(code.is_some()));
                set(&mut base, "Reference", jstr(code.clone().unwrap_or_default()));
                set(&mut base, "Flag", jstr(if code.is_some() { String::new() } else { rec.group.clone() }));
                set(&mut base, "Include_In_Average", serde_json::Value::Bool(true));
                set(&mut base, "Comment", jstr("imported U–Pb data only (no EPMA data)"));
                set(&mut base, "Mineral", jstr(mineral));
                set(&mut base, "No_EPMA", serde_json::Value::Bool(true));
                set(&mut base, "Age_Ma", blank());
                set(&mut base, "Age_2s_Ma", blank());
                set(&mut base, "Below_DL", blank());
                let up = chemchron_core::upb::upb_update_for_row(&base, rec, &p.source, Some(&session));
                for (k, v) in up {
                    base.insert(k, v);
                }
                for (k, v) in chemchron_core::upb::compute_upb_fields(&base, &decay, dp) {
                    base.insert(k, v);
                }
                app.data.rows.push(base);
                n_new += 1;
            }
        }
        summary.push(format!(
            "U–Pb: {} analyses linked, {} new rows, {} unmatched{}",
            matched.len(),
            if add { unmatched.len() } else { 0 },
            if add { 0 } else { unmatched.len() },
            if grouped > 0 { format!(", sample group set on {grouped}") } else { String::new() }
        ));
    }
    if let Some(imp) = &p.trace {
        let labels: Vec<String> = imp.records.iter().map(|r| r.label.clone()).collect();
        let (matched, unmatched) = chemchron_core::trace::match_labels(&app.data.rows, &labels, &app.settings.label_rules);
        for (i, ri) in &matched {
            let up = chemchron_core::trace::trace_update_for_row(&app.data.rows[*i], &imp.records[*ri], &p.source);
            for (k, v) in up {
                app.data.rows[*i].insert(k, v);
            }
        }
        let mut new_te = 0;
        if add {
            for ri in &unmatched {
                let rec: &TraceRecord = &imp.records[*ri];
                let label = crate::link::spot_label(&rec.label);
                // a row just created from the U–Pb data of the same spot
                if let Some(i) = app.data.rows.iter().position(|r| chemchron_core::trace::normalize_label(&text(r, "Label")) == rec.key && truthy(r, "No_EPMA")) {
                    let up = chemchron_core::trace::trace_update_for_row(&app.data.rows[i], rec, &p.source);
                    for (k, v) in up {
                        app.data.rows[i].insert(k, v);
                    }
                    continue;
                }
                let mut base = Row::new();
                set(&mut base, "Label", jstr(label));
                set(&mut base, "Row_ID", jstr(chemchron_core::processing::new_row_id()));
                set(&mut base, "Is_Reference", serde_json::Value::Bool(false));
                set(&mut base, "Reference", blank());
                set(&mut base, "Flag", blank());
                set(&mut base, "Comment", jstr("imported trace elements only (no EPMA data)"));
                set(&mut base, "Include_In_Average", serde_json::Value::Bool(true));
                set(&mut base, "Mineral", jstr(&mineral_default));
                set(&mut base, "No_EPMA", serde_json::Value::Bool(true));
                set(&mut base, "Age_Ma", blank());
                set(&mut base, "Age_2s_Ma", blank());
                set(&mut base, "Below_DL", blank());
                let up = chemchron_core::trace::trace_update_for_row(&base, rec, &p.source);
                for (k, v) in up {
                    base.insert(k, v);
                }
                app.data.rows.push(base);
                new_te += 1;
            }
        }
        n_new += new_te;
        summary.push(format!(
            "Trace elements ({}): {} analyses linked, {} new rows, {} unmatched",
            imp.elements.join(", "),
            matched.len(),
            new_te,
            if add { 0 } else { unmatched.len() }
        ));
    }
    let _ = n_new;
    app.data.refresh(&app.settings);
    if !conflicts.is_empty() {
        summary.push(format!("{} analyses already had another sample group and kept it: {}", conflicts.len(), conflicts.iter().take(10).cloned().collect::<Vec<_>>().join("; ")));
    }
    app.say(summary.join(" · "), false);
}

fn page_link(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Import laser results from files");
        ui.label(RichText::new("Laser U–Pb ratios (2SE(prop)) and trace elements are attached to EPMA analyses with the same label (case, spaces and leading zeros of the last number ignored). Spots without an EPMA analysis can be added as rows of their own — as reference materials when their label carries a reference code or one of its alternative codes (Settings > Reference materials).").color(t.dim).size(12.0));
        ui.add_space(6.0);
        section(ui, &t, "From the Laser part", |ui| {
            let n_upb = app.laser.red.selections.iter().filter(|s| s.upb.is_some()).count();
            let n_te = app.laser.red.selections.iter().filter(|s| s.te.iter().any(|v| v.is_some())).count();
            ui.label(format!("Laser session: {}{} — {} spots with U–Pb ratios, {} with trace elements.", app.laser.session.name, if app.laser.is_demo() { " (simulated demo data)" } else { "" }, n_upb, n_te));
            ui.checkbox(&mut app.combine.include_laser_refs, "Include reference-material spots (needed for the U–Pb uncertainty model and long-term log)");
            if ui.button("Read the laser results").clicked() {
                plan_laser(app);
            }
        });
        section(ui, &t, "From a file", |ui| {
            ui.horizontal(|ui| {
                if ui.button("U–Pb ratios (final ratios with 2SE and ρ)…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Tables", &["csv", "txt", "xlsx", "xlsm", "ods"]).pick_file() {
                        load_file(app, p, true);
                    }
                }
                if ui.button("Trace elements (ppm table)…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Tables", &["csv", "txt", "xlsx", "xlsm", "ods"]).pick_file() {
                        load_file(app, p, false);
                    }
                }
            });
            if app.combine.file.path.is_some() {
                file_setup(app, ui);
            }
        });
        if app.combine.pending.is_some() {
            pending_section(app, ui);
        }
    });
}

fn file_setup(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let mut reload = false;
    let mut replan = false;
    {
        let f = &mut app.combine.file;
        ui.label(RichText::new(f.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()).strong());
        ui.horizontal_wrapped(|ui| {
            if f.sheets.len() > 1 {
                ui.label("Sheet");
                egui::ComboBox::from_id_salt("sheet").selected_text(f.sheet.clone()).show_ui(ui, |ui| {
                    for s in f.sheets.clone() {
                        if ui.selectable_value(&mut f.sheet, s.clone(), &s).clicked() {
                            reload = true;
                        }
                    }
                });
            }
            ui.label("Header row");
            let mut hr = f.header_row + 1;
            if ui.add(egui::DragValue::new(&mut hr).range(1..=f.table.len().max(1))).changed() {
                f.header_row = hr - 1;
                replan = true;
            }
            let header = f.table.get(f.header_row).cloned().unwrap_or_default();
            ui.label("Label column");
            egui::ComboBox::from_id_salt("labelcol").selected_text(header.get(f.label_col).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                for (i, h) in header.iter().enumerate() {
                    if ui.selectable_value(&mut f.label_col, i, h).clicked() {
                        replan = true;
                    }
                }
            });
            if f.upb {
                ui.label("Sample group column");
                let cur = f.group_col.and_then(|g| header.get(g).cloned()).unwrap_or("(none)".into());
                egui::ComboBox::from_id_salt("groupcol").selected_text(cur).show_ui(ui, |ui| {
                    if ui.selectable_value(&mut f.group_col, None, "(none)").clicked() {
                        replan = true;
                    }
                    for (i, h) in header.iter().enumerate() {
                        if ui.selectable_value(&mut f.group_col, Some(i), h).clicked() {
                            replan = true;
                        }
                    }
                });
            } else {
                ui.label("Unit when not in the header");
                for u in ["ppm", "ppb", "wt%"] {
                    if ui.selectable_value(&mut f.unit, u.to_string(), u).clicked() {
                        replan = true;
                    }
                }
            }
        });
        ui.label(RichText::new(format!("{} rows in the table", f.table.len().saturating_sub(f.header_row + 1))).color(t.dim).size(12.0));
    }
    if reload {
        if let Some(p) = app.combine.file.path.clone() {
            let sheet = app.combine.file.sheet.clone();
            if let Ok(tb) = chemchron_core::table::read_table(&p, Some(&sheet)) {
                app.combine.file.table = tb;
            }
            replan = true;
        }
    }
    if ui.button("Read the columns").clicked() || replan {
        plan_file(app);
    }
}

fn pending_section(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let mut apply = false;
    let mut cancel = false;
    let p = app.combine.pending.as_ref().unwrap();
    let rows = &app.data.rows;
    section(ui, &t, &format!("Ready to link: {}", p.source), |ui| {
        if let Some((recs, dups)) = &p.upb {
            let labels: Vec<String> = recs.iter().map(|r| r.label.clone()).collect();
            let (m, u) = chemchron_core::trace::match_labels(rows, &labels, &app.settings.label_rules);
            ui.horizontal(|ui| {
                stat(ui, &t, "U–Pb spots", &recs.len().to_string(), t.text, 100.0);
                stat(ui, &t, "Matched to analyses", &m.len().to_string(), t.ok, 140.0);
                stat(ui, &t, "Without EPMA analysis", &u.len().to_string(), if u.is_empty() { t.text } else { t.warn }, 150.0);
            });
            if !dups.is_empty() {
                ui.label(RichText::new(format!("Duplicate labels, first one used: {}", dups.join(", "))).color(t.warn).size(12.0));
            }
            if !u.is_empty() {
                let prefixes: Vec<(String, String)> = app.settings.label_codes();
                let list: Vec<String> = u
                    .iter()
                    .take(40)
                    .map(|i| {
                        let l = &recs[*i].label;
                        match reference_of_record(app, &recs[*i], &prefixes) {
                            Some(c) => format!("{l} > reference {c}"),
                            None => l.clone(),
                        }
                    })
                    .collect();
                ui.label(RichText::new(format!("Without EPMA analysis: {}{}", list.join(", "), if u.len() > 40 { " …" } else { "" })).size(12.0).color(t.dim));
            }
        }
        if let Some(imp) = &p.trace {
            let labels: Vec<String> = imp.records.iter().map(|r| r.label.clone()).collect();
            let (m, u) = chemchron_core::trace::match_labels(rows, &labels, &app.settings.label_rules);
            ui.horizontal(|ui| {
                stat(ui, &t, "Trace-element spots", &imp.records.len().to_string(), t.text, 130.0);
                stat(ui, &t, "Matched", &m.len().to_string(), t.ok, 90.0);
                stat(ui, &t, "Without EPMA analysis", &u.len().to_string(), if u.is_empty() { t.text } else { t.warn }, 150.0);
            });
            ui.label(format!("Elements: {}", imp.elements.join(", ")));
            if !imp.bdl_counts.is_empty() {
                let b: Vec<String> = imp.bdl_counts.iter().map(|(k, v)| format!("{k} {v}×")).collect();
                ui.label(RichText::new(format!("Below detection limit (kept, flagged): {}", b.join(", "))).size(12.0).color(t.dim));
            }
            if !imp.duplicates.is_empty() {
                ui.label(RichText::new(format!("Duplicate labels, first one used: {}", imp.duplicates.join(", "))).color(t.warn).size(12.0));
            }
        }
        if !p.ignored.is_empty() {
            ui.label(RichText::new(format!("Columns not used: {}", p.ignored.join(", "))).size(12.0).color(t.dim));
        }
    });
    let c = &mut app.combine;
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut c.add_unmatched, "Add spots without EPMA analysis as new rows");
        ui.label("as");
        egui::ComboBox::from_id_salt("newmin").selected_text(c.new_mineral.clone()).show_ui(ui, |ui| {
            for m in MINERAL_NAMES {
                ui.selectable_value(&mut c.new_mineral, m.to_string(), m);
            }
        });
        ui.label("LA-ICP-MS session");
        ui.add(egui::TextEdit::singleline(&mut c.session).desired_width(180.0));
    });
    ui.horizontal(|ui| {
        apply = ui.button(RichText::new("Link to the analyses").strong()).clicked();
        cancel = ui.button("Cancel").clicked();
    });
    if apply {
        apply_pending(app);
    }
    if cancel {
        app.combine.pending = None;
    }
}

// ---- tables ----------------------------------------------------------------------------------

/// Weighted mean: chemical ages or U–Pb ages, in two tabs.
fn page_weighted_mean(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let mut v = app.combine.wm_upb;
    ui.horizontal(|ui| {
        segmented(ui, &t, &mut v, &[(false, "Chemical ages (EPMA)"), (true, "U–Pb ages (laser)")]);
    });
    app.combine.wm_upb = v;
    ui.add_space(4.0);
    if v {
        crate::combine_plots::page_upb_mean(app, ui);
    } else {
        crate::epma_pages::page_weighted_mean(app, ui);
    }
}

/// Tabs of the Data page.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DataTab {
    #[default]
    Geochronology,
    Composition,
    Import,
}

/// All analyses with their linked laser data, in Geochronology and Mineral
/// composition tabs; column groups fold.
fn page_data(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.horizontal(|ui| {
        ui.heading("Data");
        ui.add_space(12.0);
        let mut tab = app.combine.data_tab;
        if segmented(ui, &t, &mut tab, &[(DataTab::Geochronology, "Geochronology"), (DataTab::Composition, "Mineral composition"), (DataTab::Import, "Import laser results from files")]) {
            app.combine.data_tab = tab;
        }
        if app.combine.data_tab != DataTab::Import {
            ui.separator();
            let mut v = app.combine.show_refs;
            if segmented(ui, &t, &mut v, &[(false, "Samples"), (true, "Reference materials")]) {
                app.combine.show_refs = v;
            }
        }
    });
    if app.data.rows.is_empty() && app.combine.data_tab != DataTab::Import {
        ui.add_space(8.0);
        ui.label(RichText::new("No analyses yet. Import EPMA data in the EPMA part, send laser results from LA-ICP-MS > Export (Send to Combined results), or import laser results from files (tab above).").color(t.dim));
        return;
    }
    let composition = match app.combine.data_tab {
        DataTab::Import => {
            page_link(app, ui);
            return;
        }
        DataTab::Composition => true,
        DataTab::Geochronology => false,
    };
    let refs_view = app.combine.show_refs;
    let cols = if composition { columns::mineral_columns(&app.data.active, &app.data.trace, refs_view) } else { columns::age_columns(refs_view, true) };
    let cols: Vec<String> = if app.settings.show_uncertainty_columns { cols } else { cols.into_iter().filter(|c| !c.ends_with("_2s_wt%") && !c.ends_with("_DL_wt%")).collect() };
    let subset: Vec<usize> = app.data.rows.iter().enumerate().filter(|(_, r)| truthy(r, "Is_Reference") == refs_view).map(|(i, _)| i).collect();
    {
        let st = data_table(app, composition, refs_view);
        st.block_of = Some(if composition { columns::mineral_block } else { columns::geochronology_block });
        st.edit_labels = true;
    }
    table_page(app, ui, &format!("combine_table_{composition}_{refs_view}"), cols, subset, &move |a: &mut ChemApp| data_table(a, composition, refs_view));
}

fn data_table(app: &mut ChemApp, composition: bool, refs_view: bool) -> &mut TableState {
    match (composition, refs_view) {
        (false, false) => &mut app.combine.table,
        (false, true) => &mut app.combine.ref_table,
        (true, false) => &mut app.combine.comp_table,
        (true, true) => &mut app.combine.comp_ref_table,
    }
}

pub fn summary_columns(refs: bool, upb: bool) -> Vec<String> {
    let mut v: Vec<String> = if refs { columns::s(&["Reference", "Mineral", "N"]) } else { columns::s(&["Flag", "Mineral", "N"]) };
    v.extend(columns::s(&["Mean_Age_Ma", "2s_Ma", "WeightedMean_Age_Ma", "WeightedMean_2s_Ma", "MSWD"]));
    if refs {
        v.extend(columns::s(&["Accepted_Age_Ma", "Accepted_2s_Ma", "Diff_Ma", "Diff_pct"]));
    }
    if upb {
        v.extend(columns::s(&["UPb_N", "UPb_N_disc", "UPb_Mean_Ma", "UPb_2s_Ma", "UPb_WM_Ma", "UPb_WM_2s_Ma", "UPb_MSWD"]));
        if refs {
            v.extend(columns::s(&["UPb_Accepted_Ma", "UPb_Accepted_2s_Ma", "UPb_Diff_Ma", "UPb_Diff_pct"]));
        }
    }
    v
}

pub fn overviews(app: &ChemApp, all_types: bool) -> (Vec<Row>, Vec<Row>, bool) {
    let upb = chemchron_core::overview::any_upb(&app.data.rows);
    let opts = chemchron_core::pipeline::upb_options(&app.settings, all_types);
    let o = if upb { Some(&opts) } else { None };
    let refs = app.settings.refs();
    let s = &app.settings;
    let mut ro = chemchron_core::overview::reference_overview(&app.data.rows, &refs, &s.decimals, s.reject_outliers, s.rejection_sigma, o);
    let mut fo = chemchron_core::overview::flag_overview(&app.data.rows, &s.decimals, s.reject_outliers, s.rejection_sigma, o);
    for r in ro.iter_mut() {
        let id = format!("ref:{}", text(r, "Reference"));
        set(r, "Row_ID", jstr(id));
        // colour like the data tables: within / outside the accepted age
        let within = match (num(r, "WeightedMean_Age_Ma").or(num(r, "Mean_Age_Ma")), num(r, "Accepted_Age_Ma")) {
            (Some(a), Some(acc)) => {
                let s2 = num(r, "WeightedMean_2s_Ma").or(num(r, "2s_Ma")).unwrap_or(0.0);
                let a2 = num(r, "Accepted_2s_Ma").unwrap_or(0.0);
                Some((a - acc).abs() <= (s2 * s2 + a2 * a2).sqrt())
            }
            _ => None,
        };
        set(r, "Is_Reference", serde_json::Value::Bool(true));
        if let Some(w) = within {
            set(r, "Ref_Within_2s", jstr(if w { chemchron_core::reference::WITHIN_2S } else { chemchron_core::reference::OUTSIDE_2S }));
        }
    }
    for r in fo.iter_mut() {
        let id = format!("flag:{}:{}", text(r, "Flag"), text(r, "Mineral"));
        set(r, "Row_ID", jstr(id));
    }
    (ro, fo, upb)
}

/// Group means of every oxide (wt%) and element (ppm), per reference
/// material or sample group and mineral; with `sd` also 2 SD columns.
/// Analyses left out of averages and values below the detection limit don't count.
pub fn composition_overview(app: &ChemApp, refs: bool, sd: bool) -> (Vec<Row>, Vec<String>) {
    let all_cols = columns::mineral_columns(&app.data.active, &app.data.trace, refs);
    let fields: Vec<String> = all_cols
        .into_iter()
        .filter(|c| (c.ends_with("_wt%") && !c.ends_with("_2s_wt%") && !c.ends_with("_DL_wt%")) || (c.ends_with("_ppm") && !c.ends_with("_2s_ppm") && !c.ends_with("_LOD_ppm")))
        .collect();
    let key = if refs { "Reference" } else { "Flag" };
    let mut groups: Vec<(String, String)> = Vec::new();
    for r in &app.data.rows {
        if truthy(r, "Is_Reference") != refs || !flag(r, "Include_In_Average", true) {
            continue;
        }
        let g = (text(r, key), text(r, "Mineral"));
        if !g.0.is_empty() && !groups.contains(&g) {
            groups.push(g);
        }
    }
    let mut cols: Vec<String> = vec![key.to_string(), "Mineral".into(), "N".into()];
    let mut used_fields: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (g, m) in &groups {
        let members: Vec<&Row> = app.data.rows.iter().filter(|r| truthy(r, "Is_Reference") == refs && flag(r, "Include_In_Average", true) && text(r, key) == *g && text(r, "Mineral") == *m).collect();
        let mut row = Row::new();
        set(&mut row, key, jstr(g));
        set(&mut row, "Mineral", jstr(m));
        set(&mut row, "N", jnum(members.len() as f64));
        for f in &fields {
            let ox = f.strip_suffix("_wt%");
            let vals: Vec<f64> = members
                .iter()
                .filter(|r| ox.is_none_or(|o| !chemchron_core::sessions::below_dl_set(r).iter().any(|b| b == o)))
                .filter_map(|r| num(r, f))
                .filter(|v| v.is_finite() && (ox.is_some() || *v > 0.0))
                .collect();
            if vals.is_empty() {
                continue;
            }
            let n = vals.len() as f64;
            let mean = vals.iter().sum::<f64>() / n;
            let dp = if ox.is_some() { 3 } else { 1 };
            let round = |x: f64| (x * 10f64.powi(dp)).round() / 10f64.powi(dp);
            set(&mut row, f, jnum(round(mean)));
            if sd && vals.len() > 1 {
                let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
                set(&mut row, &format!("{f} 2SD"), jnum(round(2.0 * var.sqrt())));
            }
            if !used_fields.contains(f) {
                used_fields.push(f.clone());
            }
        }
        out.push(row);
    }
    for f in fields.iter().filter(|f| used_fields.contains(f)) {
        cols.push(f.clone());
        if sd {
            cols.push(format!("{f} 2SD"));
        }
    }
    (out, cols)
}

/// Summary views: ages, or group composition means.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SummaryView {
    #[default]
    Ages,
    Composition,
}

fn page_summary(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let mut view = app.combine.summary_view;
    ui.horizontal(|ui| {
        ui.heading("Summary");
        ui.add_space(10.0);
        segmented(ui, &t, &mut view, &[(SummaryView::Ages, "Ages"), (SummaryView::Composition, "Group composition (means)")]);
        if view == SummaryView::Composition {
            ui.checkbox(&mut app.combine.summary_sd, "Show 2 SD");
        }
    });
    app.combine.summary_view = view;
    if view == SummaryView::Composition {
        page_summary_composition(app, ui);
        return;
    }
    let (ro, fo, upb) = overviews(app, false);
    ui.horizontal_wrapped(|ui| {
        if upb {
            ui.label("U–Pb age");
            let mut a = app.settings.upb.summary_age_type.clone();
            egui::ComboBox::from_id_salt("sum_age").selected_text(chemchron_core::upbstats::age_type_label(&a)).show_ui(ui, |ui| {
                for ty in chemchron_core::upbstats::AGE_TYPES {
                    ui.selectable_value(&mut a, ty.to_string(), chemchron_core::upbstats::age_type_label(ty));
                }
            });
            if a != app.settings.upb.summary_age_type {
                app.settings.upb.summary_age_type = a;
                app.save_settings();
                app.data.refresh(&app.settings);
            }
        }
        let mut rej = app.settings.reject_outliers;
        if ui.checkbox(&mut rej, format!("Reject outliers ({:.1}σ) in weighted means", app.settings.rejection_sigma)).changed() {
            app.settings.reject_outliers = rej;
            app.save_settings();
        }
    });
    ui.label(RichText::new("Chemical columns use analyses included in averages (PbO below d.l. left out); U–Pb columns use each laser spot once, without discordant or imprecise spots. Weighted-mean 2σ include the systematic term.").color(t.dim).size(12.0));
    let flag_colors = app.group_colors();
    let dp = app.settings.decimals;
    let look = Look { t: &t, dp: &dp, flag_colors: &flag_colors, show_bdl_values: true };
    let h = (ui.available_height() - 60.0) / 2.0;
    ui.label(RichText::new(format!("Reference materials ({}) — right-click a row to plot that material", ro.len())).strong());
    let cols = summary_columns(true, upb);
    let idx: Vec<usize> = (0..ro.len()).collect();
    let mut chosen: Option<(crate::epma::PlotTarget, Vec<GroupPick>)> = None;
    let pick_ref = |r: &Row| GroupPick::Reference(text(r, "Reference"));
    let pick_flag = |r: &Row| GroupPick::Flag(text(r, "Flag"), text(r, "Mineral"));
    ui.allocate_ui(egui::vec2(ui.available_width(), h.max(140.0)), |ui| {
        let mut menu = |ui: &mut Ui, targets: &[usize]| {
            if let Some(p) = summary_menu(ui, upb) {
                chosen = Some((p, targets.iter().map(|i| pick_ref(&ro[*i])).collect()));
            }
        };
        rowtable::show_menu(ui, "sum_refs", &ro, &idx, &cols, &mut app.combine.sum_refs, &look, Some(&mut menu));
    });
    ui.add_space(6.0);
    ui.label(RichText::new(format!("Sample groups ({}) — right-click a row to plot that group", fo.len())).strong());
    let cols = summary_columns(false, upb);
    let idx: Vec<usize> = (0..fo.len()).collect();
    let mut menu = |ui: &mut Ui, targets: &[usize]| {
        if let Some(p) = summary_menu(ui, upb) {
            chosen = Some((p, targets.iter().map(|i| pick_flag(&fo[*i])).collect()));
        }
    };
    rowtable::show_menu(ui, "sum_flags", &fo, &idx, &cols, &mut app.combine.sum_flags, &look, Some(&mut menu));
    if let Some((p, groups)) = chosen {
        plot_groups(app, p, groups);
    }
}

fn page_summary_composition(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.label(RichText::new("Mean oxide (wt%, EPMA) and element (ppm, laser or imported) contents of each group; analyses left out of averages and values below the detection limit don't count.").color(t.dim).size(12.0));
    let flag_colors = app.group_colors();
    let dp = app.settings.decimals;
    let look = Look { t: &t, dp: &dp, flag_colors: &flag_colors, show_bdl_values: true };
    let sd = app.combine.summary_sd;
    let (ro, rcols) = composition_overview(app, true, sd);
    let (fo, fcols) = composition_overview(app, false, sd);
    let h = (ui.available_height() - 60.0) / 2.0;
    ui.label(RichText::new(format!("Reference materials ({})", ro.len())).strong());
    let idx: Vec<usize> = (0..ro.len()).collect();
    ui.allocate_ui(egui::vec2(ui.available_width(), h.max(140.0)), |ui| {
        rowtable::show(ui, "sum_comp_refs", &ro, &idx, &rcols, &mut app.combine.sum_comp_refs, &look);
    });
    ui.add_space(6.0);
    ui.label(RichText::new(format!("Sample groups ({})", fo.len())).strong());
    let idx: Vec<usize> = (0..fo.len()).collect();
    rowtable::show(ui, "sum_comp_flags", &fo, &idx, &fcols, &mut app.combine.sum_comp_flags, &look);
}

fn summary_menu(ui: &mut Ui, upb: bool) -> Option<crate::epma::PlotTarget> {
    use crate::epma::PlotTarget as P;
    let mut out = None;
    for (label, p, on) in [
        ("Isochron (Th)", P::IsochronTh, true),
        ("Isochron (U)", P::IsochronU, true),
        ("Weighted mean (chemical age)", P::WeightedMean, true),
        ("Weighted mean (U–Pb age)", P::UpbMean, upb),
        ("Concordia (Tera–Wasserburg)", P::ConcordiaTw, upb),
        ("Concordia (Wetherill)", P::ConcordiaW, upb),
        ("REE diagram (median + IQR of the selected groups)", P::Ree, true),
        ("Ternary plot (Mnz–Cher–Ht)", P::Ternary, true),
        ("Exchange vector plot", P::Exchange, true),
    ] {
        if ui.add_enabled(on, egui::Button::new(label)).clicked() {
            out = Some(p);
            ui.close();
        }
    }
    out
}

/// Opens a plot page for whole groups chosen on the Summary.
fn plot_groups(app: &mut ChemApp, p: crate::epma::PlotTarget, groups: Vec<GroupPick>) {
    use crate::epma::PlotTarget as P;
    let Some(first) = groups.first().cloned() else { return };
    match p {
        P::IsochronTh | P::IsochronU => {
            app.part = crate::app::Part::Combine;
            app.combine.page = Page::Isochron;
            app.epma.iso_group = first;
            app.epma.iso_u = p == P::IsochronU;
        }
        P::WeightedMean => {
            app.part = crate::app::Part::Combine;
            app.combine.page = Page::WeightedMean;
            app.epma.wm_group = first;
            app.combine.wm_upb = false;
        }
        P::UpbMean => {
            app.combine.page = Page::UpbMean;
            app.combine.iwm_group = first;
        }
        P::ConcordiaTw | P::ConcordiaW => {
            app.combine.page = Page::Concordia;
            app.combine.conc_group = first;
            app.combine.conc_wetherill = p == P::ConcordiaW;
        }
        P::Ree => {
            app.combine.page = Page::Ree;
            app.combine.ree_sel = groups;
            app.combine.ree_groups = true;
        }
        P::Ternary | P::Exchange | P::XY => {
            app.combine.page = Page::Composition;
            app.combine.comp_sel = groups;
            app.combine.comp_kind = if p == P::Ternary { 0 } else { 1 };
        }
    }
}
