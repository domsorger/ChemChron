// SPDX-License-Identifier: GPL-3.0-or-later
//! EPMA part: import, the Geochronology and Mineral composition tables and
//! their row actions. Plots, uncertainty model, reference materials and
//! settings are in `epma_pages`.

use crate::app::{frame_part, nav, ChemApp};
use crate::rowtable::{self, Look, TableState};
use chemchron_core::columns;
use chemchron_core::constants::*;
use chemchron_core::row::*;
use chemchron_core::stats::mineral_of;
use egui::{RichText, Ui};
use std::path::PathBuf;
use chemchron_laser::plot::View;
use chemchron_laser::theme::{section, segmented, stat};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Data,
    Geochronology,
    Composition,
}

impl Page {
    pub fn from_name(n: &str) -> Option<Page> {
        Some(match n.to_lowercase().as_str() {
            "data" => Page::Data,
            "geochronology" | "ages" => Page::Geochronology,
            "composition" | "mineral" => Page::Composition,
            _ => return None,
        })
    }
}

/// Which analyses a plot or statistic uses.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum GroupPick {
    #[default]
    None,
    Flag(String, String),
    Reference(String),
    Selection,
}

impl GroupPick {
    pub fn label(&self) -> String {
        match self {
            GroupPick::None => "–".into(),
            GroupPick::Flag(f, m) => format!("{f} ({})", mineral_abbreviation(m)),
            GroupPick::Reference(r) => format!("{r} (reference)"),
            GroupPick::Selection => "Rows selected in the table".into(),
        }
    }
}

#[derive(Default)]
pub struct EpmaState {
    pub page: Page,
    pub show_refs: bool,
    pub geo_table: TableState,
    pub ref_table: TableState,
    pub comp_table: TableState,
    pub comp_ref_table: TableState,
    pub messages: Vec<String>,
    pub iso_group: GroupPick,
    pub iso_u: bool,
    pub iso_view: View,
    pub wm_group: GroupPick,
    pub wm_view: View,
    pub wm_hide_excluded: bool,
    pub new_group: String,
    pub comment: String,
    pub session_text: String,
    pub paste_open: bool,
    pub paste_text: String,
    pub paste_append: bool,
    pub add_point_open: bool,
    pub add_point: Vec<(String, String)>,
    pub unc_mineral: String,
    /// Typed in Settings > General > Sample groups.
    pub new_group_name: String,
    pub rename_from: String,
    pub rename_to: String,
    pub menu: MenuText,
}

pub fn show(app: &mut ChemApp, ui: &mut Ui) {
    frame_part(app, ui, "epma", nav_ui, |app, ui| match app.epma.page {
        Page::Data => page_data(app, ui),
        Page::Geochronology => page_tables(app, ui, false),
        Page::Composition => page_tables(app, ui, true),
    });
    paste_window(app, ui.ctx());
    add_point_window(app, ui.ctx());
}

fn nav_ui(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let has = app.data.rows.iter().any(|r| !truthy(r, "No_EPMA"));
    let ok = |b: bool| Some(if b { t.ok } else { t.border });
    let outside = app.data.rows.iter().filter(|r| text(r, "Ref_Within_2s") == chemchron_core::reference::OUTSIDE_2S).count();
    let pages = [
        (Page::Data, "Data", ok(has), true),
        (Page::Geochronology, "Geochronology", if outside > 0 { Some(t.warn) } else { ok(has) }, true),
        (Page::Composition, "Mineral composition", ok(has), true),
    ];
    nav(ui, &t, &mut app.epma.page, &pages);
    ui.add_space(12.0);
    ui.label(RichText::new("Electron microprobe Th–U–Pb dating. Plots, linked laser data and the long-term logs are in Combined results.").color(t.dim).size(11.5));
}

pub fn import_files(app: &mut ChemApp, paths: Vec<PathBuf>, append: bool) {
    if paths.is_empty() {
        return;
    }
    let list: Vec<(PathBuf, String)> =
        paths.iter().map(|p| (p.clone(), chemchron_core::sessions::session_name_from_path(&p.to_string_lossy()))).collect();
    let existing = if append { app.data.rows.clone() } else { Vec::new() };
    let (new_rows, active, messages) = chemchron_core::pipeline::import_epma(&list, &existing, &app.settings);
    app.data.checkpoint(if append { "Append EPMA data" } else { "Import EPMA data" });
    let mut all_active = if append { app.data.active.clone() } else { Vec::new() };
    for o in active {
        if !all_active.contains(&o) {
            all_active.push(o);
        }
    }
    let n = new_rows.len();
    if append {
        app.data.rows.extend(new_rows);
    } else {
        app.data.rows = new_rows;
    }
    app.data.active = chemchron_core::pipeline::order_active(&all_active);
    app.data.refresh(&app.settings);
    let warn = messages.iter().any(|m| !m.contains("note:"));
    app.say(
        format!(
            "{} {n} point(s) from {} file(s). Table now has {} rows.{}",
            if append { "Added" } else { "Loaded" },
            list.len(),
            app.data.rows.len(),
            if messages.is_empty() { String::new() } else { format!(" {} message(s) on the Data page.", messages.len()) }
        ),
        warn,
    );
    app.epma.messages = messages;
}

fn page_data(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("EPMA data");
        ui.label(RichText::new("Raw microprobe exports (Point, Comment, …(Mass%)) or ChemChron CSV tables with Label, ThO2, UO2, PbO. Each file becomes one EPMA session.").color(t.dim));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Import EPMA data (.csv)…").on_hover_text("Replace the table with these files").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("EPMA data", &["csv", "txt"]).pick_files() {
                    import_files(app, p, false);
                }
            }
            if ui.button("Append EPMA data…").on_hover_text("Add the files to the table").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("EPMA data", &["csv", "txt"]).pick_files() {
                    import_files(app, p, true);
                }
            }
            if ui.button("Paste from clipboard…").clicked() {
                app.epma.paste_open = true;
            }
            if ui.button("Add single point…").clicked() {
                app.epma.add_point_open = true;
                if app.epma.add_point.is_empty() {
                    let mut f: Vec<(String, String)> = ["Label", "ThO2", "UO2", "PbO", "sigma_ThO2", "sigma_UO2", "sigma_PbO", "Flag", "Comment", "EPMA_Session"]
                        .iter()
                        .map(|k| (k.to_string(), String::new()))
                        .collect();
                    for o in &app.data.active {
                        f.push((o.clone(), String::new()));
                    }
                    app.epma.add_point = f;
                }
            }
            ui.separator();
            if ui.button("Open project…").on_hover_text("A ChemChron project").clicked() {
                app.open_project_dialog();
            }
        });
        ui.add_space(8.0);
        let rows = &app.data.rows;
        let n = rows.iter().filter(|r| !truthy(r, "No_EPMA")).count();
        let refs = rows.iter().filter(|r| truthy(r, "Is_Reference") && !truthy(r, "No_EPMA")).count();
        let bdl = rows.iter().filter(|r| chemchron_core::stats::pbo_below_detection_limit(r)).count();
        let outside = rows.iter().filter(|r| text(r, "Ref_Within_2s") == chemchron_core::reference::OUTSIDE_2S).count();
        ui.horizontal_wrapped(|ui| {
            stat(ui, &t, "EPMA analyses", &n.to_string(), t.text, 120.0);
            stat(ui, &t, "Reference analyses", &refs.to_string(), t.text, 120.0);
            stat(ui, &t, "Reference outside 2σ", &outside.to_string(), if outside > 0 { t.warn } else { t.ok }, 140.0);
            stat(ui, &t, "PbO below d.l.", &bdl.to_string(), if bdl > 0 { t.warn } else { t.text }, 120.0);
            stat(ui, &t, "Oxides measured", &(app.data.active.len() + 3).to_string(), t.text, 120.0);
        });
        ui.add_space(8.0);
        if !app.epma.messages.is_empty() {
            section(ui, &t, "Import messages", |ui| {
                for m in app.epma.messages.iter().take(60) {
                    ui.label(RichText::new(m).color(if m.contains("note:") { t.dim } else { t.warn }).size(12.0));
                }
                if app.epma.messages.len() > 60 {
                    ui.label(format!("… and {} more", app.epma.messages.len() - 60));
                }
            });
        }
        section(ui, &t, "Oxides in this project", |ui| {
            let mut v: Vec<String> = PARENT_OXIDES.iter().map(|s| s.to_string()).collect();
            v.extend(app.data.active.iter().cloned());
            ui.label(v.join(", "));
            ui.label(RichText::new("Al₂O₃ and ZrO₂ are contamination monitors only and never enter the formula (ZrO₂ does in zircon).").color(t.dim).size(12.0));
        });
        sessions_section(app, ui);
    });
}

fn sessions_section(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    section(ui, &t, "EPMA sessions", |ui| {
        let sessions = chemchron_core::sessions::sessions_in(&app.data.rows, false);
        if sessions.is_empty() {
            ui.label(RichText::new("No EPMA data yet.").color(t.dim));
        }
        egui::Grid::new("sessions_grid").striped(true).show(ui, |ui| {
            for h in ["Session", "Analyses", "References", "Random excess in use (monazite)"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for s in &sessions {
                let members: Vec<&Row> = app.data.rows.iter().filter(|r| chemchron_core::sessions::epma_session(r) == *s && !truthy(r, "No_EPMA")).collect();
                ui.label(chemchron_core::sessions::display_session(s));
                ui.label(members.len().to_string());
                ui.label(members.iter().filter(|r| truthy(r, "Is_Reference")).count().to_string());
                let ex = app
                    .data
                    .models
                    .as_ref()
                    .and_then(|m| m.chem.get(DEFAULT_MINERAL))
                    .and_then(|m| m.excess_by_session.get(s))
                    .map(|(p, src)| format!("{p:.2} % ({src})"))
                    .unwrap_or_default();
                ui.label(ex);
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Rename session");
            egui::ComboBox::from_id_salt("rename_from").selected_text(chemchron_core::sessions::display_session(&app.epma.rename_from).to_string()).show_ui(ui, |ui| {
                for s in &sessions {
                    ui.selectable_value(&mut app.epma.rename_from, s.clone(), chemchron_core::sessions::display_session(s));
                }
            });
            ui.label("to");
            ui.text_edit_singleline(&mut app.epma.rename_to);
            if ui.button("Rename").clicked() && !app.epma.rename_to.trim().is_empty() {
                let (from, to) = (app.epma.rename_from.clone(), app.epma.rename_to.trim().to_string());
                app.data.checkpoint("Rename EPMA session");
                for r in app.data.rows.iter_mut() {
                    if chemchron_core::sessions::epma_session(r) == from && !truthy(r, "No_EPMA") {
                        set(r, "EPMA_Session", jstr(&to));
                    }
                }
                app.data.refresh(&app.settings);
                app.epma.rename_to.clear();
            }
        });
    });
}

fn paste_window(app: &mut ChemApp, ctx: &egui::Context) {
    if !app.epma.paste_open {
        return;
    }
    let mut open = true;
    let mut go = false;
    egui::Window::new("Paste EPMA data").open(&mut open).default_width(640.0).show(ctx, |ui| {
        ui.label("Paste a table copied from a spreadsheet or the microprobe software (header row included). Tab, ; or , separated; decimal commas are converted.");
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut app.epma.paste_text).desired_rows(12).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
        let table = chemchron_core::epma::clipboard_table(&app.epma.paste_text);
        if let Some(h) = table.first() {
            let (fmt, used, ignored) = chemchron_core::epma::describe_pasted_columns(h);
            if fmt.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(235, 104, 110), "Not recognised: needs Label, ThO2, UO2, PbO, or the instrument header Point, Comment, …(Mass%).");
            } else {
                let kind = if fmt == "export" { "instrument export" } else { "ChemChron" };
                ui.label(format!("{} rows, {kind} format. Used: {}", table.len() - 1, used.join(", ")));
                if !ignored.is_empty() {
                    ui.label(RichText::new(format!("Ignored: {}", ignored.join(", "))).size(12.0));
                }
            }
        }
        ui.checkbox(&mut app.epma.paste_append, "Append to the table (otherwise replace it)");
        go = ui.button("Import").clicked();
    });
    if go {
        let table = chemchron_core::epma::clipboard_table(&app.epma.paste_text);
        let dir = std::env::temp_dir().join(format!("chemchron-paste-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("pasted data.csv");
        if std::fs::write(&p, chemchron_core::epma::table_to_csv(&table)).is_ok() {
            let append = app.epma.paste_append;
            import_files(app, vec![p.clone()], append);
            let _ = std::fs::remove_file(&p);
        }
        app.epma.paste_open = false;
    } else {
        app.epma.paste_open = open;
    }
}

fn add_point_window(app: &mut ChemApp, ctx: &egui::Context) {
    if !app.epma.add_point_open {
        return;
    }
    let mut open = true;
    let mut add = false;
    egui::Window::new("Add single point").open(&mut open).show(ctx, |ui| {
        ui.label("Oxides in wt%, sigma_* as 1σ wt% (blank: 1 % Th/U, 2 % Pb).");
        egui::Grid::new("add_point").show(ui, |ui| {
            for (k, v) in app.epma.add_point.iter_mut() {
                ui.label(columns::display_name(k));
                ui.add(egui::TextEdit::singleline(v).desired_width(140.0));
                ui.end_row();
            }
        });
        add = ui.button("Add").clicked();
    });
    if add {
        let mut row = Row::new();
        for (k, v) in &app.epma.add_point {
            set(&mut row, k, jstr(v.trim()));
        }
        let refs = app.settings.refs();
        let model = chemchron_core::pipeline::chem_model(&app.data.rows, &app.settings);
        let ctx2 = chemchron_core::pipeline::context(&app.settings, &refs, &app.data.active, &model);
        match chemchron_core::processing::process_row(&row, &ctx2) {
            Ok(r) => {
                app.data.checkpoint("Add single point");
                app.data.rows.push(r);
                app.data.refresh(&app.settings);
                app.say("Point added.", false);
                for (_, v) in app.epma.add_point.iter_mut() {
                    v.clear();
                }
            }
            Err(e) => app.say(e, true),
        }
    }
    app.epma.add_point_open = open && !add;
}

// ---- row actions ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Action {
    Group(Option<String>),
    Reference(String),
    Mineral(&'static str),
    Reidentify,
    EpmaSession(String),
    UpbSession(String),
    Include(bool),
    UpbInclude(bool),
    ToggleInclude,
    ToggleUpbInclude,
    Comment(String),
    RemoveUpb,
    RemoveTrace,
    Delete,
    Copy,
    Plot(PlotTarget),
}

impl Action {
    fn name(&self) -> String {
        match self {
            Action::Group(None) => "Clear sample group".into(),
            Action::Group(Some(g)) => format!("Assign sample group {g}"),
            Action::Reference(c) => format!("Assign reference {c}"),
            Action::Mineral(m) => format!("Set mineral {m}"),
            Action::Reidentify => "Reprocess mineral ID".into(),
            Action::EpmaSession(_) => "Set EPMA session".into(),
            Action::UpbSession(_) => "Set U–Pb session".into(),
            Action::ToggleInclude => "Include / leave out".into(),
            Action::ToggleUpbInclude => "Include / leave out of U–Pb averages".into(),
            Action::Include(true) => "Include in average".into(),
            Action::Include(false) => "Leave out of average".into(),
            Action::UpbInclude(true) => "Include in U–Pb averages".into(),
            Action::UpbInclude(false) => "Leave out of U–Pb averages".into(),
            Action::Comment(_) => "Set comment".into(),
            Action::RemoveUpb => "Remove U–Pb data".into(),
            Action::RemoveTrace => "Remove trace elements".into(),
            Action::Delete => "Delete rows".into(),
            Action::Copy => "Copy rows".into(),
            Action::Plot(_) => "Plot".into(),
        }
    }
}

fn remove_keys(r: &mut Row, pred: impl Fn(&str) -> bool) {
    let keys: Vec<String> = r.keys().filter(|k| pred(k)).cloned().collect();
    for k in keys {
        r.remove(&k);
    }
}

fn apply_action(a: &Action, r: &mut Row, refs: &chemchron_core::reference::Refs, dp: &DecimalPlaces, active: &[String], th: &MineralIdThresholds) {
    use serde_json::Value::Bool;
    match a {
        Action::Group(g) => {
            set(r, "Is_Reference", Bool(false));
            set(r, "Reference", blank());
            set(r, "Flag", g.as_ref().map(jstr).unwrap_or_else(blank));
            chemchron_core::reference::recompute_ref_fields_for_row(r, refs, dp);
        }
        Action::Reference(c) => {
            set(r, "Is_Reference", Bool(true));
            set(r, "Reference", jstr(c));
            set(r, "Flag", blank());
            chemchron_core::reference::recompute_ref_fields_for_row(r, refs, dp);
        }
        Action::Mineral(m) => {
            if truthy(r, "No_EPMA") {
                set(r, "Mineral", jstr(*m));
            } else {
                chemchron_core::formula::recompute_mineral_formula_for_row(r, active, m, dp);
            }
        }
        Action::Reidentify => {
            if !truthy(r, "No_EPMA") {
                let w = chemchron_core::formula::oxide_wt_from_processed(r, active);
                let m = chemchron_core::formula::identify_mineral(&w, th);
                chemchron_core::formula::recompute_mineral_formula_for_row(r, active, m, dp);
            }
        }
        Action::EpmaSession(s) => {
            if !truthy(r, "No_EPMA") {
                set(r, "EPMA_Session", jstr(s))
            }
        }
        Action::UpbSession(s) => {
            if chemchron_core::upb::has_upb(r) {
                set(r, "UPb_Session", jstr(s))
            }
        }
        Action::Include(on) => set(r, "Include_In_Average", Bool(*on)),
        Action::UpbInclude(true) => {
            r.remove("UPb_Include");
        }
        Action::UpbInclude(false) => {
            if chemchron_core::upb::has_upb(r) {
                set(r, "UPb_Include", Bool(false))
            }
        }
        Action::Comment(c) => set(r, "Comment", jstr(c)),
        Action::RemoveUpb => remove_keys(r, |k| k.starts_with("UPb_")),
        Action::RemoveTrace => remove_keys(r, |k| chemchron_core::trace::trace_field(k).is_some() || k == "TE_Below_DL" || k == "TE_Source"),
        Action::Delete | Action::Copy | Action::Plot(_) | Action::ToggleInclude | Action::ToggleUpbInclude => {}
    }
}

/// A cell typed in the table. A label changes only its row; a comment or
/// session typed into a selected row goes to every selected row.
pub fn edit_cell(app: &mut ChemApp, i: usize, col: &str, new: String) {
    if col == "Label" && new.is_empty() {
        return;
    }
    let rid = text(&app.data.rows[i], "Row_ID");
    let mut targets = vec![i];
    if col != "Label" {
        for st in [&app.epma.geo_table, &app.epma.ref_table, &app.epma.comp_table, &app.epma.comp_ref_table, &app.combine.table, &app.combine.ref_table, &app.combine.comp_table, &app.combine.comp_ref_table] {
            if st.selected.contains(&rid) && st.selected.len() > 1 {
                targets = app.data.rows.iter().enumerate().filter(|(_, r)| st.selected.contains(&text(r, "Row_ID"))).map(|(k, _)| k).collect();
                break;
            }
        }
    }
    if targets.iter().all(|k| text(&app.data.rows[*k], col) == new) {
        return;
    }
    let name = chemchron_core::columns::display_name(col);
    app.data.checkpoint(&format!("Edit {name}"));
    for k in &targets {
        set(&mut app.data.rows[*k], col, serde_json::Value::String(new.clone()));
    }
    app.data.refresh(&app.settings);
    if col == "Label" {
        app.say(format!("Label changed to {new}. Settings > Reference materials > Recognise references in labels again re-checks reference materials."), false);
    } else {
        app.say(format!("{name} set on {} rows.", targets.len()), false);
    }
}

/// Where "Plot" in the row menu sends the selected rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlotTarget {
    IsochronTh,
    IsochronU,
    WeightedMean,
    UpbMean,
    ConcordiaTw,
    ConcordiaW,
    Ree,
    Ternary,
    Exchange,
    XY,
}

/// Choices for the row menus, taken from the project before drawing.
pub struct MenuCtx {
    pub flags: Vec<String>,
    pub refs: chemchron_core::reference::Refs,
    pub has_upb: bool,
}

/// Text typed into the menus (kept between frames in `EpmaState`).
#[derive(Default, Clone)]
pub struct MenuText {
    pub new_group: String,
    pub comment: String,
    pub session: String,
}

pub fn menu_ctx(app: &ChemApp) -> MenuCtx {
    // groups of the project and those made in Settings (no analyses yet)
    let mut flags: Vec<String> = app.data.rows.iter().map(|r| text(r, "Flag")).filter(|f| !f.is_empty()).chain(app.settings.flag_colors.keys().cloned()).collect();
    flags.sort();
    flags.dedup();
    MenuCtx { flags, refs: app.settings.refs(), has_upb: app.data.rows.iter().any(chemchron_core::upb::has_upb) }
}

/// A menu that stays open while you type into a field in it (egui menus
/// otherwise close on the first click, also a click into a text field).
pub fn typing_menu(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    egui::containers::menu::MenuButton::new(label)
        .config(egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
        .ui(ui, add);
}

fn group_items(ui: &mut Ui, c: &MenuCtx, m: &mut MenuText, a: &mut Option<Action>) {
    if ui.button("(no group)").clicked() {
        *a = Some(Action::Group(None));
        ui.close();
    }
    for f in &c.flags {
        if ui.button(f).clicked() {
            *a = Some(Action::Group(Some(f.clone())));
            ui.close();
        }
    }
    ui.separator();
    let field = ui.add(egui::TextEdit::singleline(&mut m.new_group).hint_text("new group").desired_width(130.0));
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if (ui.button("Assign new group").clicked() || enter) && !m.new_group.trim().is_empty() {
        *a = Some(Action::Group(Some(m.new_group.trim().to_string())));
        ui.close();
    }
}

fn reference_items(ui: &mut Ui, c: &MenuCtx, a: &mut Option<Action>) {
    if c.refs.is_empty() {
        ui.label("No reference materials defined yet");
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for code in c.refs.keys() {
            let age = chemchron_core::reference::ref_num(&c.refs, code, "age").map(|x| format!(" ({x} Ma)")).unwrap_or_default();
            if ui.button(format!("{code}{age}")).clicked() {
                *a = Some(Action::Reference(code.clone()));
                ui.close();
            }
        }
    });
}

fn mineral_items(ui: &mut Ui, a: &mut Option<Action>) {
    for m in MINERAL_NAMES {
        if ui.button(format!("{m} ({})", mineral_abbreviation(m))).clicked() {
            *a = Some(Action::Mineral(m));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Identify again from the composition").clicked() {
        *a = Some(Action::Reidentify);
        ui.close();
    }
}


fn plot_items(ui: &mut Ui, c: &MenuCtx, a: &mut Option<Action>) {
    let mut item = |ui: &mut Ui, label: &str, p: PlotTarget, enabled: bool| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            *a = Some(Action::Plot(p));
            ui.close();
        }
    };
    item(ui, "Isochron (Th)", PlotTarget::IsochronTh, true);
    item(ui, "Isochron (U)", PlotTarget::IsochronU, true);
    item(ui, "Weighted mean (chemical age)", PlotTarget::WeightedMean, true);
    ui.separator();
    item(ui, "Weighted mean (U–Pb age)", PlotTarget::UpbMean, c.has_upb);
    item(ui, "Concordia (Tera–Wasserburg)", PlotTarget::ConcordiaTw, c.has_upb);
    item(ui, "Concordia (Wetherill)", PlotTarget::ConcordiaW, c.has_upb);
    ui.separator();
    item(ui, "Ternary plot (Mnz–Cher–Ht)", PlotTarget::Ternary, true);
    item(ui, "Exchange vector plot", PlotTarget::Exchange, true);
    item(ui, "X–Y plot", PlotTarget::XY, true);
    item(ui, "REE diagram", PlotTarget::Ree, true);
}

fn more_items(ui: &mut Ui, _m: &mut MenuText, a: &mut Option<Action>) {
    if ui.button("Remove U–Pb data").clicked() {
        *a = Some(Action::RemoveUpb);
        ui.close();
    }
    if ui.button("Remove imported trace elements").clicked() {
        *a = Some(Action::RemoveTrace);
        ui.close();
    }
}

/// The right-click menu of a table row (the same choices as the bar above the table).
pub fn context_items(ui: &mut Ui, c: &MenuCtx, m: &mut MenuText) -> Option<Action> {
    let mut a = None;
    egui::containers::menu::SubMenuButton::new("Assign to sample group")
        .config(egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
        .ui(ui, |ui| group_items(ui, c, m, &mut a));
    ui.menu_button("Assign as reference material", |ui| reference_items(ui, c, &mut a));
    ui.menu_button("Mineral", |ui| mineral_items(ui, &mut a));
    ui.separator();
    if ui.button("Include / leave out of averages").on_hover_text("Switches: left out when all are included, otherwise included").clicked() {
        a = Some(Action::ToggleInclude);
        ui.close();
    }
    if c.has_upb && ui.button("Include / leave out of U–Pb averages").clicked() {
        a = Some(Action::ToggleUpbInclude);
        ui.close();
    }
    ui.menu_button("More", |ui| more_items(ui, m, &mut a));
    ui.separator();
    ui.menu_button("Plot", |ui| plot_items(ui, c, &mut a));
    ui.separator();
    if ui.button("Copy rows").clicked() {
        a = Some(Action::Copy);
        ui.close();
    }
    if ui.button("Delete rows").clicked() {
        a = Some(Action::Delete);
        ui.close();
    }
    a
}

/// The bar above a table: the same actions for the selected rows.
pub fn row_actions(app: &mut ChemApp, ui: &mut Ui, sel: &[usize], cols: &[String]) {
    let t = app.theme();
    let c = menu_ctx(app);
    let mut m = app.epma.menu.clone();
    let mut a: Option<Action> = None;
    let any = !sel.is_empty();
    let all_in = any && sel.iter().all(|i| flag(&app.data.rows[*i], "Include_In_Average", true));
    let upb_rows: Vec<usize> = sel.iter().copied().filter(|i| chemchron_core::upb::has_upb(&app.data.rows[*i])).collect();
    let all_upb_in = !upb_rows.is_empty() && upb_rows.iter().all(|i| chemchron_core::upb::included_in_upb(&app.data.rows[*i]));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(if any { format!("{} selected:", sel.len()) } else { "Select rows (click, Shift / Ctrl + click):".to_string() }).strong());
        ui.add_enabled_ui(any, |ui| {
            typing_menu(ui, "Sample group", |ui| group_items(ui, &c, &mut m, &mut a));
            ui.menu_button("Reference", |ui| reference_items(ui, &c, &mut a));
            ui.menu_button("Mineral", |ui| mineral_items(ui, &mut a));
            let label = if all_in { "Leave out of averages" } else { "Include in averages" };
            if ui.button(label).on_hover_text("Switches the selected rows between included in and left out of averages, isochrons and the Summary").clicked() {
                a = Some(Action::Include(!all_in));
            }
            if !upb_rows.is_empty() {
                let label = if all_upb_in { "Leave out of U–Pb averages" } else { "Include in U–Pb averages" };
                if ui.button(label).clicked() {
                    a = Some(Action::UpbInclude(!all_upb_in));
                }
            }
            ui.menu_button("Plot", |ui| plot_items(ui, &c, &mut a));
            ui.menu_button("More", |ui| {
                more_items(ui, &mut m, &mut a);
                ui.separator();
                if ui.button("Copy rows").clicked() {
                    a = Some(Action::Copy);
                    ui.close();
                }
                if ui.button("Delete rows").clicked() {
                    a = Some(Action::Delete);
                    ui.close();
                }
            });
        });
        ui.label(RichText::new("Double-click a label, comment or session to edit it (comments and sessions: for all selected rows).").color(t.dim).size(11.5));
    });
    app.epma.menu = m;
    if let Some(a) = a {
        apply_row_action(app, ui.ctx(), a, sel, cols);
    }
}

/// Sends the rows to a plot page with "rows selected in the table" chosen.
fn plot_rows(app: &mut ChemApp, p: PlotTarget, sel: &[usize]) {
    let ids: std::collections::BTreeSet<String> = sel.iter().map(|i| text(&app.data.rows[*i], "Row_ID")).collect();
    for st in [&mut app.epma.geo_table, &mut app.epma.ref_table, &mut app.epma.comp_table, &mut app.epma.comp_ref_table, &mut app.combine.table, &mut app.combine.ref_table, &mut app.combine.comp_table, &mut app.combine.comp_ref_table] {
        st.selected = ids.clone();
    }
    use crate::combine::Page as C;
    match p {
        PlotTarget::IsochronTh | PlotTarget::IsochronU => {
            app.part = crate::app::Part::Combine;
            app.combine.page = C::Isochron;
            app.epma.iso_group = GroupPick::Selection;
            app.epma.iso_u = p == PlotTarget::IsochronU;
        }
        PlotTarget::WeightedMean => {
            app.part = crate::app::Part::Combine;
            app.combine.page = C::WeightedMean;
            app.epma.wm_group = GroupPick::Selection;
            app.combine.wm_upb = false;
            // a selection is shown in the order of the table
            app.settings.rank_by_age = false;
        }
        PlotTarget::UpbMean => {
            app.part = crate::app::Part::Combine;
            app.combine.page = C::UpbMean;
            app.combine.iwm_group = GroupPick::Selection;
        }
        PlotTarget::ConcordiaTw | PlotTarget::ConcordiaW => {
            app.part = crate::app::Part::Combine;
            app.combine.page = C::Concordia;
            app.combine.conc_group = GroupPick::Selection;
            app.combine.conc_wetherill = p == PlotTarget::ConcordiaW;
        }
        PlotTarget::Ree | PlotTarget::Ternary | PlotTarget::Exchange | PlotTarget::XY => {
            app.part = crate::app::Part::Combine;
            if p == PlotTarget::Ree {
                app.combine.page = C::Ree;
                app.combine.ree_sel = vec![GroupPick::Selection];
            } else {
                app.combine.page = C::Composition;
                app.combine.comp_sel = vec![GroupPick::Selection];
                app.combine.comp_kind = match p {
                    PlotTarget::Ternary => 0,
                    PlotTarget::Exchange => 1,
                    _ => 2,
                };
            }
        }
    }
}

pub fn apply_row_action(app: &mut ChemApp, ctx: &egui::Context, a: Action, sel: &[usize], cols: &[String]) {
    let a = match a {
        Action::ToggleInclude => Action::Include(!sel.iter().all(|i| flag(&app.data.rows[*i], "Include_In_Average", true))),
        Action::ToggleUpbInclude => {
            let u: Vec<&usize> = sel.iter().filter(|i| chemchron_core::upb::has_upb(&app.data.rows[**i])).collect();
            Action::UpbInclude(!(!u.is_empty() && u.iter().all(|i| chemchron_core::upb::included_in_upb(&app.data.rows[**i]))))
        }
        other => other,
    };
    match a {
        Action::Copy => {
            ctx.copy_text(rowtable::copy_text(&app.data.rows, sel, cols, &app.settings.decimals));
            app.say(format!("Copied {} rows (tab-separated, paste into a spreadsheet).", sel.len()), false);
            return;
        }
        Action::Plot(p) => {
            plot_rows(app, p, sel);
            return;
        }
        _ => {}
    }
    let what = a.name();
    app.data.checkpoint(&what);
    if let Action::Delete = a {
        let rows = std::mem::take(&mut app.data.rows);
        app.data.rows = rows.into_iter().enumerate().filter(|(i, _)| !sel.contains(i)).map(|(_, r)| r).collect();
        app.data.refresh(&app.settings);
        app.say(format!("Deleted {} rows (Undo brings them back).", sel.len()), false);
        return;
    }
    let refs = app.settings.refs();
    let dp = app.settings.decimals;
    let active = app.data.active.clone();
    let th = app.settings.thresholds.clone();
    for i in sel {
        apply_action(&a, &mut app.data.rows[*i], &refs, &dp, &active, &th);
    }
    app.data.refresh(&app.settings);
    app.say(format!("{what}: {} rows.", sel.len()), false);
}

// ---- tables -----------------------------------------------------------------------------------

pub fn table_state(app: &mut ChemApp, composition: bool, refs_view: bool) -> &mut TableState {
    match (composition, refs_view) {
        (false, false) => &mut app.epma.geo_table,
        (false, true) => &mut app.epma.ref_table,
        (true, false) => &mut app.epma.comp_table,
        (true, true) => &mut app.epma.comp_ref_table,
    }
}

/// A table page: filter, row actions, the table; returns nothing, edits go through `Data`.
pub fn table_page(app: &mut ChemApp, ui: &mut Ui, id: &str, cols: Vec<String>, subset: Vec<usize>, pick: &dyn Fn(&mut ChemApp) -> &mut TableState) {
    let t = app.theme();
    let (vis, sel) = {
        let rows = std::mem::take(&mut app.data.rows);
        let dp = app.settings.decimals;
        let crit = app.settings.upb.criteria();
        let st = pick(app);
        st.criteria = Some(crit);
        let mut groups: Vec<String> = subset.iter().map(|i| rowtable::group_of(&rows[*i])).collect();
        groups.sort();
        groups.dedup();
        rowtable::filter_bar_with(ui, st, &cols, &t, id, &groups);
        let vis = rowtable::visible(&rows, &subset, st, &cols, &dp);
        let sel = rowtable::selected_indices(&rows, st);
        app.data.rows = rows;
        (vis, sel)
    };
    ui.label(RichText::new(format!("{} of {} shown", vis.len(), subset.len())).color(t.dim).size(12.0));
    row_actions(app, ui, &sel, &cols);
    ui.add_space(4.0);
    let flag_colors = app.group_colors();
    let dp = app.settings.decimals;
    let look = Look { t: &t, dp: &dp, flag_colors: &flag_colors, show_bdl_values: app.settings.show_actual_below_dl };
    let mctx = menu_ctx(app);
    let rows = std::mem::take(&mut app.data.rows);
    let mut mtext = app.epma.menu.clone();
    let mut chosen: Option<(Action, Vec<usize>)> = None;
    let mut menu = |ui: &mut Ui, targets: &[usize]| {
        if let Some(a) = context_items(ui, &mctx, &mut mtext) {
            chosen = Some((a, targets.to_vec()));
        }
    };
    let ev = rowtable::show_menu(ui, id, &rows, &vis, &cols, pick(app), &look, Some(&mut menu));
    app.table_order = pick(app).order.clone();
    app.data.rows = rows;
    app.epma.menu = mtext;
    if let Some((a, targets)) = chosen {
        apply_row_action(app, ui.ctx(), a, &targets, &cols);
        return;
    }
    if let Some((i, col, new)) = ev.edited {
        edit_cell(app, i, &col, new);
    }
    if let Some(i) = ev.toggled_include {
        toggle_include(app, i);
    }
    if let Some(i) = ev.toggled_upb_include {
        app.data.checkpoint("Include in U–Pb averages");
        if chemchron_core::upb::included_in_upb(&app.data.rows[i]) {
            set(&mut app.data.rows[i], "UPb_Include", serde_json::Value::Bool(false));
        } else {
            app.data.rows[i].remove("UPb_Include");
        }
        app.data.refresh(&app.settings);
    }
}

fn page_tables(app: &mut ChemApp, ui: &mut Ui, composition: bool) {
    let t = app.theme();
    ui.horizontal(|ui| {
        ui.heading(if composition { "Mineral composition" } else { "Geochronology" });
        ui.add_space(12.0);
        let mut v = app.epma.show_refs;
        if segmented(ui, &t, &mut v, &[(false, "Samples"), (true, "Reference materials")]) {
            app.epma.show_refs = v;
        }
    });
    let refs_view = app.epma.show_refs;
    let cols = if composition { columns::mineral_columns(&app.data.active, &app.data.trace, refs_view) } else { columns::age_columns(refs_view, false) };
    let cols: Vec<String> = if app.settings.show_uncertainty_columns { cols } else { cols.into_iter().filter(|c| !c.ends_with("_2s_wt%") && !c.ends_with("_DL_wt%")).collect() };
    let subset: Vec<usize> =
        app.data.rows.iter().enumerate().filter(|(_, r)| truthy(r, "Is_Reference") == refs_view && !truthy(r, "No_EPMA")).map(|(i, _)| i).collect();
    let id = format!("epma_table_{composition}_{refs_view}");
    {
        let st = table_state(app, composition, refs_view);
        st.block_of = Some(if composition { columns::mineral_block } else { columns::geochronology_block });
        st.edit_labels = true;
    }
    table_page(app, ui, &id, cols, subset, &move |a: &mut ChemApp| table_state(a, composition, refs_view));
}

// ---- groups for plots ----------------------------------------------------------------------

pub fn group_choices(rows: &[Row], with_refs: bool) -> Vec<GroupPick> {
    let mut v = Vec::new();
    for r in rows {
        let g = if truthy(r, "Is_Reference") {
            let c = text(r, "Reference");
            if !with_refs || c.is_empty() {
                continue;
            }
            GroupPick::Reference(c)
        } else {
            let f = text(r, "Flag");
            if f.is_empty() {
                continue;
            }
            GroupPick::Flag(f, mineral_of(r))
        };
        if !v.contains(&g) {
            v.push(g);
        }
    }
    v.push(GroupPick::Selection);
    v
}

pub fn group_rows(rows: &[Row], g: &GroupPick, selection: &TableState) -> Vec<usize> {
    match g {
        GroupPick::None => Vec::new(),
        GroupPick::Flag(f, m) => {
            rows.iter().enumerate().filter(|(_, r)| !truthy(r, "Is_Reference") && text(r, "Flag") == *f && mineral_of(r) == *m).map(|(i, _)| i).collect()
        }
        GroupPick::Reference(c) => rows.iter().enumerate().filter(|(_, r)| truthy(r, "Is_Reference") && text(r, "Reference") == *c).map(|(i, _)| i).collect(),
        GroupPick::Selection => rowtable::selected_indices(rows, selection),
    }
}

pub fn group_picker(ui: &mut Ui, id: &str, current: &mut GroupPick, choices: &[GroupPick]) {
    if *current == GroupPick::None || !choices.contains(current) {
        if let Some(c) = choices.first() {
            *current = c.clone();
        }
    }
    egui::ComboBox::from_id_salt(id).selected_text(current.label()).width(230.0).show_ui(ui, |ui| {
        for c in choices {
            ui.selectable_value(current, c.clone(), c.label());
        }
    });
}

pub fn toggle_include(app: &mut ChemApp, i: usize) {
    app.data.checkpoint("Include / leave out");
    let on = flag(&app.data.rows[i], "Include_In_Average", true);
    set(&mut app.data.rows[i], "Include_In_Average", serde_json::Value::Bool(!on));
    app.data.refresh(&app.settings);
}
