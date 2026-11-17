// SPDX-License-Identifier: GPL-3.0-or-later
//! The ChemChron window: one bar to switch between the Laser, EPMA and
//! Combine parts, the project file that holds all three, and undo for the
//! analysis table.

use crate::{combine, epma};
use chemchron_core::pipeline::{self, Models};
use chemchron_core::row::*;
use chemchron_core::settings::{self, EpmaProject, Settings};
use egui::{Color32, Context, Margin, RichText, Stroke, Ui};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use chemchron_laser::app::TraceApp;
use chemchron_laser::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Laser,
    Epma,
    Combine,
    /// Program-wide settings (working directory, reference materials, ...).
    Settings,
}

impl Part {
    pub fn from_name(n: &str) -> Option<Part> {
        match n.to_lowercase().as_str() {
            "laser" | "la" | "laicpms" => Some(Part::Laser),
            "epma" => Some(Part::Epma),
            "combine" | "combined" => Some(Part::Combine),
            "settings" => Some(Part::Settings),
            _ => None,
        }
    }
}

/// The analyses of the project (EPMA rows, with linked laser data) and their undo history.
pub struct Data {
    pub rows: Vec<Row>,
    pub active: Vec<String>,
    pub trace: Vec<String>,
    pub models: Option<Models>,
    undo: Vec<(Vec<Row>, Vec<String>, String)>,
    redo: Vec<(Vec<Row>, Vec<String>, String)>,
    pub unsaved: bool,
    pub revision: u64,
    /// Counts checkpoints (the one undo history follows it).
    pub edits: u64,
}

impl Default for Data {
    fn default() -> Self {
        Data { rows: Vec::new(), active: Vec::new(), trace: Vec::new(), models: None, undo: Vec::new(), redo: Vec::new(), unsaved: false, revision: 1, edits: 0 }
    }
}

/// Where an undoable change happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Data,
    Laser,
    Settings,
}

/// One undo history across the parts: Ctrl+Z takes back the latest change,
/// wherever it was made (analyses, laser session or lab settings).
#[derive(Default)]
pub struct History {
    order: Vec<Kind>,
    future: Vec<Kind>,
    seen_data: u64,
    seen_laser: u64,
    settings_undo: Vec<Settings>,
    settings_redo: Vec<Settings>,
    settings_committed: Option<Settings>,
    last_settings: Option<std::time::Instant>,
}

impl Data {
    /// Call before changing the rows; `what` names the step in the undo menu.
    pub fn checkpoint(&mut self, what: &str) {
        self.undo.push((self.rows.clone(), self.active.clone(), what.to_string()));
        self.edits += 1;
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.unsaved = true;
    }
    pub fn can_undo(&self) -> Option<&str> {
        self.undo.last().map(|u| u.2.as_str())
    }
    pub fn can_redo(&self) -> Option<&str> {
        self.redo.last().map(|u| u.2.as_str())
    }
    pub fn undo(&mut self) {
        if let Some((r, a, w)) = self.undo.pop() {
            let cur = (std::mem::replace(&mut self.rows, r), std::mem::replace(&mut self.active, a), w);
            self.redo.push(cur);
            self.unsaved = true;
        }
    }
    pub fn redo(&mut self) {
        if let Some((r, a, w)) = self.redo.pop() {
            let cur = (std::mem::replace(&mut self.rows, r), std::mem::replace(&mut self.active, a), w);
            self.undo.push(cur);
            self.unsaved = true;
        }
    }
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    /// Re-applies everything derived from the rows.
    pub fn refresh(&mut self, s: &Settings) {
        self.models = Some(pipeline::refresh(&mut self.rows, s));
        self.trace = chemchron_core::trace::trace_elements_in_rows(&self.rows);
        self.revision += 1;
    }
    pub fn index_of(&self, row_id: &str) -> Option<usize> {
        self.rows.iter().position(|r| text(r, "Row_ID") == row_id)
    }
}

pub struct ChemApp {
    pub part: Part,
    pub laser: TraceApp,
    pub data: Data,
    pub settings: Settings,
    pub epma: epma::EpmaState,
    pub combine: combine::CombineState,
    pub global: crate::global::GlobalState,
    pub refs_ui: crate::refs_page::RefsUi,
    laser_last_page: chemchron_laser::app::Page,
    pub dark: bool,
    pub project_path: Option<PathBuf>,
    pub notice: Option<(String, bool)>,
    pub help_open: bool,
    /// The figure waiting in the Save figure window, with its file name.
    pub fig_export: Option<(crate::figure::Figure, String)>,
    /// Figures gathered while pages are drawn off-screen (Export all figures).
    pub collect: Option<Vec<(String, crate::figure::Figure)>>,
    pub help_topic: usize,
    confirm_close: bool,
    allow_close: bool,
    screenshot: Option<(PathBuf, bool, Arc<Mutex<Option<Result<(), String>>>>)>,
    frames: u64,
    pub laser_dirty_seen: bool,
    pub history: History,
    /// Row_IDs of the analysis table last shown, in its order (plots of a selection follow it).
    pub table_order: Vec<String>,
    applied_dark: Option<bool>,
    /// Laser reference list as last synchronised with the lab file.
    pub rm_state: crate::refsync::SyncState,
    pub autosave: crate::autosave::AutoState,
    /// Laser sessions of the project that aren't open now.
    pub laser_waiting: Vec<crate::laser_sessions::Waiting>,
    /// --screenshot-tour: every page in turn.
    pub tour: Option<crate::tour::Tour>,
    /// Laser sample-group colours as last seen (session key, name -> colour).
    group_colours_seen: (String, std::collections::BTreeMap<String, [u8; 3]>),
}

pub const PROJECT_FORMAT: &str = "chemchron-project-2";

impl ChemApp {
    pub fn new(ctx: &Context) -> ChemApp {
        let mut laser = TraceApp::new(ctx);
        laser.embedded = true;
        let settings = Settings::load();
        let mut app = ChemApp {
            part: Part::Epma,
            laser,
            data: Data::default(),
            settings,
            epma: Default::default(),
            combine: Default::default(),
            global: Default::default(),
            refs_ui: Default::default(),
            laser_last_page: chemchron_laser::app::Page::Data,
            dark: true,
            project_path: None,
            notice: None,
            help_open: false,
            fig_export: None,
            collect: None,
            help_topic: 0,
            confirm_close: false,
            allow_close: false,
            screenshot: None,
            frames: 0,
            laser_dirty_seen: false,
            history: Default::default(),
            table_order: Vec::new(),
            applied_dark: None,
            rm_state: Default::default(),
            autosave: Default::default(),
            laser_waiting: Vec::new(),
            tour: None,
            group_colours_seen: Default::default(),
        };
        app.dark = app.settings.dark;
        app.data.refresh(&app.settings);
        app.laser.start_empty();
        app
    }

    /// One colour per sample group in every part: a laser sample group takes the
    /// lab's colour of that name (Settings > General > Sample groups); a colour
    /// changed in the laser part becomes the lab's colour; a new laser group
    /// gives the lab its colour.
    pub fn sync_group_colours(&mut self, typing: bool) {
        let key = format!("{}|{:?}|{}", self.laser.session.name, std::mem::discriminant(&self.laser.session.source), self.laser.session.files.len());
        if self.group_colours_seen.0 != key {
            self.group_colours_seen = (key, Default::default());
        }
        let mut lab_changed = false;
        let mut quiet: Vec<(u64, [u8; 3])> = Vec::new();
        let mut seen_now = std::collections::BTreeMap::new();
        for g in self.laser.session.groups.iter().filter(|g| g.kind == chemchron_laser_core::model::GroupKind::Sample) {
            let lab = self.settings.flag_colors.get(&g.name).and_then(|h| crate::plots::parse_hex(h)).map(|c| [c.r(), c.g(), c.b()]);
            let seen = self.group_colours_seen.1.get(&g.name).copied();
            let mut now = g.color;
            if seen.is_some_and(|s| s != g.color) {
                // changed on the Signal page
                if lab != Some(g.color) {
                    self.settings.flag_colors.insert(g.name.clone(), crate::plots::to_hex(egui::Color32::from_rgb(g.color[0], g.color[1], g.color[2])));
                    lab_changed = true;
                }
            } else if let Some(l) = lab {
                if l != g.color {
                    quiet.push((g.id, l));
                    now = l;
                }
            } else if !g.name.trim().is_empty() && !typing {
                // (not while a name is being typed: every half-typed name would be kept)
                self.settings.flag_colors.insert(g.name.clone(), crate::plots::to_hex(egui::Color32::from_rgb(g.color[0], g.color[1], g.color[2])));
                lab_changed = true;
            }
            seen_now.insert(g.name.clone(), now);
        }
        for (id, c) in quiet {
            self.laser.set_group_colour_quiet(id, c);
        }
        self.group_colours_seen.1 = seen_now;
        if lab_changed {
            self.save_settings_quiet();
        }
    }

    /// The laser part works out its excess as Settings > Uncertainty models >
    /// U–Pb says: from the session's reference materials or manual values, with
    /// the materials (or single ratios) left out there.
    pub fn sync_laser_excess(&mut self) {
        let mineral = self.laser_mineral();
        let s = self.settings.upb.by_mineral().remove(&mineral).unwrap_or_default();
        let manual = (s.excess_source == "manual").then(|| {
            let g = |r: &str| s.excess_manual_pct.get(r).copied().unwrap_or(0.0);
            [g("r86"), g("r86").hypot(g("r76")), g("r76"), g("r82")]
        });
        let names = |code: &str| -> Vec<String> { self.laser.session.rms.iter().filter(|m| m.name == code || m.matches_label(code)).map(|m| m.name.clone()).collect() };
        let mut excluded: Vec<String> = Vec::new();
        for code in &s.excluded_refs {
            excluded.extend(names(code));
        }
        for x in &s.excluded_ratios {
            if let Some((code, r)) = x.split_once('|') {
                let laser: &[&str] = match r {
                    "r86" => &["r68", "r75"],
                    "r76" => &["r76", "r75"],
                    "r82" => &["r82"],
                    "r75" => &["r75"],
                    _ => &[],
                };
                for n in names(code) {
                    for l in laser {
                        let e = format!("{n}|{l}");
                        if !excluded.contains(&e) {
                            excluded.push(e);
                        }
                    }
                }
            }
        }
        excluded.sort();
        self.laser.set_excess_rule(chemchron_laser_core::model::ExcessRule { manual_2s_pct: manual, excluded });
    }

    /// The mineral of the open laser session (most of its sample groups).
    pub fn laser_mineral(&self) -> String {
        let s = &self.laser.session;
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for g in s.groups.iter().filter(|g| g.kind == chemchron_laser_core::model::GroupKind::Sample) {
            *counts.entry(s.mineral_of(g.id)).or_default() += 1;
        }
        counts.into_iter().max_by_key(|x| x.1).map(|x| x.0).unwrap_or_else(|| chemchron_core::constants::DEFAULT_MINERAL.to_string())
    }

    /// The laser part shows the same totals as Combined results: it gets the
    /// lab's systematic term for the session's mineral (long-term log or manual,
    /// and the decay constants when chosen).
    pub fn sync_lab_systematic(&mut self) {
        use chemchron_core::upbstats::{decay_rel_2s_pct, AGE_TYPES};
        let mineral = self.laser_mineral();
        let o = chemchron_core::pipeline::upb_options(&self.settings, false);
        let mut pct = [0.0; 4];
        let mut source = String::new();
        for (k, t) in AGE_TYPES.iter().enumerate() {
            let (base, src) = o.systematic.by_mineral.get(&mineral).and_then(|m| m.get(*t)).cloned().unwrap_or((self.settings.upb.systematic_pct, "manual".into()));
            let dec = o.systematic.decay.as_ref().map_or(0.0, |c| decay_rel_2s_pct(t, 1000.0, c));
            pct[k] = base.hypot(dec);
            if k == 0 {
                source = format!("{mineral}: {src}{}", if dec > 0.0 { " + decay constants" } else { "" });
            }
        }
        let new = Some(chemchron_laser::app::LabSystematic { pct, source });
        if self.laser.lab_systematic != new {
            self.laser.lab_systematic = new;
        }
    }

    /// The laser session uses the lab's decay constants (Settings > Decay constants).
    pub fn sync_laser_decay(&mut self) {
        if self.laser.is_demo() {
            return;
        }
        let d = &self.settings.decay;
        let l = self.laser.session.settings.upb.decay;
        if l.l238 != d.lambda_238 || l.l235 != d.lambda_235 || l.l232 != d.lambda_232 || l.u85 != d.u238_u235 {
            let x = &mut self.laser.session.settings.upb.decay;
            x.l238 = d.lambda_238;
            x.l235 = d.lambda_235;
            x.l232 = d.lambda_232;
            x.u85 = d.u238_u235;
            self.laser.recalculate();
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::get(self.dark)
    }

    /// Colour of every sample group (chosen or automatic), the same in tables and plots.
    pub fn group_colors(&self) -> std::collections::BTreeMap<String, String> {
        crate::plots::project_colors(&self.settings.flag_colors, &self.data.rows)
    }

    pub fn say(&mut self, text: impl Into<String>, problem: bool) {
        self.notice = Some((text.into(), problem));
    }

    pub fn request_screenshot(&mut self, path: PathBuf) {
        self.screenshot = Some((path, false, Arc::new(Mutex::new(None))));
    }

    /// Saves the lab settings and records the change for Undo (quick
    /// successive changes, like dragging a number, undo together).
    pub fn save_settings(&mut self) {
        let h = &mut self.history;
        if let Some(before) = h.settings_committed.take() {
            if before != self.settings {
                let quick = h.last_settings.is_some_and(|t| t.elapsed().as_secs_f64() < 0.8) && h.order.last() == Some(&Kind::Settings);
                if !quick {
                    h.settings_undo.push(before);
                    if h.settings_undo.len() > 50 {
                        h.settings_undo.remove(0);
                    }
                    h.order.push(Kind::Settings);
                }
                h.future.clear();
                h.settings_redo.clear();
                h.last_settings = Some(std::time::Instant::now());
            }
        }
        h.settings_committed = Some(self.settings.clone());
        self.save_settings_quiet();
    }

    /// Saves the lab settings without an undo step (changes the program makes itself).
    pub fn save_settings_quiet(&mut self) {
        self.history.settings_committed = Some(self.settings.clone());
        if let Err(e) = self.settings.save() {
            self.say(format!("Settings could not be saved: {e}"), true);
        }
    }

    // ---- project files ---------------------------------------------------------

    pub fn project_name(&self) -> String {
        self.project_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().trim_end_matches(".json").trim_end_matches(".chemchron").to_string())
            .unwrap_or_else(|| "Untitled project".into())
    }

    pub fn new_project(&mut self) {
        self.data = Data::default();
        self.data.refresh(&self.settings);
        self.project_path = None;
        self.epma = Default::default();
        self.combine = Default::default();
        self.laser.start_empty();
        self.laser_waiting.clear();
        self.say("New, empty project.", false);
    }

    pub fn project_value(&self) -> Value {
        let epma = settings::project_to_value(&EpmaProject { rows: self.data.rows.clone(), active_oxides: self.data.active.clone(), meta: Default::default() });
        let laser = if self.laser.is_demo() || self.laser.is_empty() || !self.settings.laser_in_project { Value::Null } else { serde_json::to_value(&self.laser.session).unwrap_or(Value::Null) };
        json!({
            "format": PROJECT_FORMAT,
            "software": format!("ChemChron {}", env!("CARGO_PKG_VERSION")),
            "version": 1,
            "active_oxides": epma["active_oxides"],
            "rows": epma["rows"],
            "detection_rule": epma["detection_rule"],
            "laser": laser,
            "laser_sessions": if self.settings.laser_in_project { Value::Array(self.laser_waiting.iter().filter_map(|w| serde_json::to_value(&w.session).ok()).collect()) } else { Value::Array(Vec::new()) },
            "plots": crate::plot_settings::to_value(self),
            // values are kept with every decimal (rounded only when shown)
            "full_precision": true,
        })
    }

    pub fn save_project(&mut self, ask: bool) {
        let target = if ask || self.project_path.is_none() {
            rfd::FileDialog::new()
                .set_file_name(format!("{}.chemchron.json", chemchron_laser::app::sanitize(&self.project_name())))
                .add_filter("ChemChron project", &["json"])
                .save_file()
        } else {
            self.project_path.clone()
        };
        let Some(path) = target else { return };
        let path = if path.extension().is_none() { path.with_extension("json") } else { path };
        let text = serde_json::to_string_pretty(&self.project_value()).unwrap();
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.data.unsaved = false;
                if self.settings.laser_in_project {
                    self.laser.unsaved = false;
                    for w in self.laser_waiting.iter_mut() {
                        w.unsaved = false;
                    }
                }
                self.settings.add_recent(&path.to_string_lossy());
                self.save_settings();
                self.say(format!("Saved {}", path.display()), false);
                self.project_path = Some(path);
            }
            Err(e) => self.say(format!("{}: {e}", path.display()), true),
        }
    }

    pub fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("ChemChron project", &["json"]).pick_file() {
            self.open_project(&p);
        }
    }

    /// Opens a ChemChron project (older file versions are converted on loading) or a laser session.
    pub fn open_project(&mut self, path: &Path) {
        let mut plots = Value::Null;
        let r = (|| -> Result<String, String> {
            let t = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let v: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
            // laser sessions (older format names are read too)
            if v.get("format").and_then(|f| f.as_str()).is_some_and(|f| f.starts_with("chemchron-laser") || f.starts_with("tracechron")) {
                self.laser.open_blocking(&[path.to_path_buf()]);
                self.part = Part::Laser;
                return Ok(format!("Opened the laser session {}", path.display()));
            }
            let p = settings::project_from_value(&v, &pipeline::file_name(path))?;
            plots = v.get("plots").cloned().unwrap_or(Value::Null);
            let mut msg = format!("Opened {} ({} analyses)", path.display(), p.rows.len());
            self.data = Data::default();
            self.data.rows = p.rows;
            self.data.active = p.active_oxides;
            self.laser_waiting.clear();
            if let Some(Value::Array(list)) = v.get("laser_sessions") {
                for l in list {
                    match crate::link::load_laser_value(l, path.parent()) {
                        Ok((session, _)) => self.laser_waiting.push(crate::laser_sessions::Waiting { session, unsaved: false, path: None }),
                        Err(e) => msg.push_str(&format!(" — a laser session could not be reopened: {e}")),
                    }
                }
            }
            chemchron_core::processing::ensure_row_ids(&mut self.data.rows);
            // projects from before 1.2 hold values rounded to the shown decimals:
            // calculated again from the measured data, with every decimal
            if !v.get("full_precision").and_then(|x| x.as_bool()).unwrap_or(false) && !self.data.rows.is_empty() {
                self.data.rows = pipeline::reprocess_all(&self.data.rows, &self.data.active, &self.settings);
                for r in self.data.rows.iter_mut().filter(|r| chemchron_core::upb::has_upb(r)) {
                    for (k, val) in chemchron_core::upb::compute_upb_fields(r, &self.settings.decay, self.settings.decimals.age) {
                        r.insert(k, val);
                    }
                }
            }
            chemchron_core::sessions::fill_missing_upb_sessions(&mut self.data.rows);
            if let Some(l) = v.get("laser").filter(|l| !l.is_null()) {
                match crate::link::load_laser_value(l, path.parent()) {
                    Ok((session, notes)) => {
                        self.laser.session = session;
                        self.laser.recalculate();
                        self.laser.after_session_replaced();
                        if !notes.is_empty() {
                            msg.push_str(&format!(" — laser data: {}", notes.join(" ")));
                        }
                    }
                    Err(e) => msg.push_str(&format!(" — the laser session could not be reopened: {e}")),
                }
            }
            Ok(msg)
        })();
        match r {
            Ok(m) => {
                self.data.refresh(&self.settings);
                self.data.unsaved = false;
                self.project_path = Some(path.to_path_buf());
                self.settings.add_recent(&path.to_string_lossy());
                self.save_settings();
                self.epma = Default::default();
                self.combine = Default::default();
                if !plots.is_null() {
                    crate::plot_settings::apply(self, &plots);
                }
                self.say(m, false);
            }
            Err(e) => self.say(e, true),
        }
    }

    /// Recomputes every row (after decay constants, decimals, references or thresholds changed).
    pub fn reprocess(&mut self, why: &str) {
        if self.data.rows.is_empty() {
            return;
        }
        self.data.checkpoint(why);
        self.data.rows = pipeline::reprocess_all(&self.data.rows, &self.data.active, &self.settings);
        self.data.refresh(&self.settings);
        self.say(format!("Recalculated {} analyses ({why}).", self.data.rows.len()), false);
    }

    pub fn unsaved(&self) -> bool {
        self.data.unsaved || self.laser_unsaved()
    }

    /// The laser session has changes that a project save doesn't keep or hasn't kept yet.
    pub fn laser_unsaved(&self) -> bool {
        (self.laser.unsaved && !self.laser.is_demo() && !self.laser.is_empty()) || self.laser_waiting.iter().any(|w| w.unsaved)
    }

    // ---- frame ---------------------------------------------------------------

    pub fn ui_root(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        self.laser.dark = self.dark;
        self.sync_laser_decay();
        self.sync_lab_systematic();
        self.sync_laser_excess();
        if self.laser.request_demo {
            self.laser.request_demo = false;
            crate::demo::load(self);
        }
        if self.laser.request_push {
            self.laser.request_push = false;
            crate::combine::link_laser_now(self);
            self.laser.sent_revision = Some(self.laser.revision);
            self.part = Part::Combine;
            self.combine.page = crate::combine::Page::Data;
        }
        if self.laser.request_is_from_epma {
            self.laser.request_is_from_epma = false;
            let (m, problem) = crate::link::internal_standard_from_epma(self);
            self.laser.say(m, problem);
        }
        let t = self.theme();
        if self.applied_dark != Some(self.dark) {
            // The controls of every part (tables, text boxes, menus) follow the theme.
            t.apply(&ctx);
            self.applied_dark = Some(self.dark);
            if self.settings.dark != self.dark {
                self.settings.dark = self.dark;
                self.save_settings();
            }
        }
        crate::refsync::sync(self);
        let typing = ctx.memory(|m| m.focused().is_some());
        self.sync_group_colours(typing);
        // The laser part's reference list is the lab list in Settings.
        if self.laser.page == chemchron_laser::app::Page::References {
            self.laser.page = self.laser_last_page;
            self.part = Part::Settings;
            self.global.page = crate::global::Page::References;
        } else {
            self.laser_last_page = self.laser.page;
        }
        self.track_history();
        self.shortcuts(&ctx);
        self.dropped_files(&ctx);
        egui::Panel::top("cc_top")
            .exact_size(48.0)
            .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(12, 7)).stroke(Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.top_bar(ui));
        match self.part {
            Part::Laser => {
                if !self.laser.is_demo() || !self.laser_waiting.is_empty() {
                    egui::Panel::top("cc_laser_sessions")
                        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(12, 4)).stroke(Stroke::new(1.0, t.border)))
                        .show(ui, |ui| crate::laser_sessions::strip(self, ui));
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg)).show(ui, |ui| self.laser.ui_root(ui));
            }
            Part::Epma => epma::show(self, ui),
            Part::Combine => combine::show(self, ui),
            Part::Settings => crate::global::show(self, ui),
        }
        if self.help_open {
            crate::help::window(self, &ctx);
        }
        crate::dialogs::windows(self, &ctx);
        crate::figure::export_window(self, &ctx);
        self.close_dialog(&ctx);
        if self.screenshot.is_none() && self.tour.is_none() {
            crate::autosave::tick(self, &ctx);
        }
        self.drive_screenshot(&ctx);
        if crate::tour::drive(self, &ctx) {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn shortcuts(&mut self, ctx: &Context) {
        let typing = ctx.memory(|m| m.focused().is_some());
        let (undo, redo, save) = ctx.input_mut(|i| {
            let redo = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
            let undo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
            let save = i.consume_key(egui::Modifiers::COMMAND, egui::Key::S);
            (undo, redo, save)
        });
        if !typing {
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
        }
        if save {
            self.save_project(false);
        }
    }

    /// Files dropped on the window: a project opens; in LA-ICP-MS, data files
    /// or folders open as a laser session; elsewhere tables (CSV, Excel, ODS,
    /// text) are imported as EPMA data (appended when there is data already).
    fn dropped_files(&mut self, ctx: &Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(0xd09f)));
            painter.rect_filled(screen, 0, Color32::from_black_alpha(150));
            let what = if self.part == Part::Laser { "laser data or a saved session" } else { "EPMA tables (CSV, Excel, ODS) or a ChemChron project" };
            painter.text(screen.center(), egui::Align2::CENTER_CENTER, format!("Drop {what} here"), egui::FontId::proportional(22.0), Color32::WHITE);
        }
        let paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if paths.is_empty() {
            return;
        }
        let is_project = |p: &Path| p.to_string_lossy().to_lowercase().ends_with(".json");
        if let Some(p) = paths.iter().find(|p| is_project(p) && self.part != Part::Laser) {
            let p = p.clone();
            self.open_project(&p);
            return;
        }
        if self.part == Part::Laser {
            self.laser.start_import(paths);
            return;
        }
        let tables: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| ["csv", "txt", "xlsx", "xls", "ods", "tsv"].contains(&e.to_string_lossy().to_lowercase().as_str())))
            .collect();
        if tables.is_empty() {
            self.say("Drop EPMA tables (CSV, Excel, ODS) or a ChemChron project here; laser data goes to the LA-ICP-MS part.", true);
            return;
        }
        let append = !self.data.rows.is_empty();
        crate::epma::import_files(self, tables, append);
        if self.part == Part::Settings {
            self.part = Part::Epma;
        }
    }

    /// Notes new changes of the analyses and the laser session in the one history.
    fn track_history(&mut self) {
        let h = &mut self.history;
        if h.settings_committed.is_none() {
            h.settings_committed = Some(self.settings.clone());
        }
        if self.data.edits != h.seen_data {
            h.seen_data = self.data.edits;
            h.order.push(Kind::Data);
            h.future.clear();
        }
        if self.laser.edits != h.seen_laser {
            h.seen_laser = self.laser.edits;
            h.order.push(Kind::Laser);
            h.future.clear();
        }
        if h.order.len() > 400 {
            h.order.drain(..100);
        }
    }

    fn settings_changed_by_undo(&mut self) {
        self.history.settings_committed = Some(self.settings.clone());
        if let Err(e) = self.settings.save() {
            self.say(format!("Settings could not be saved: {e}"), true);
        }
        let refs = self.settings.refs();
        let dp = self.settings.decimals;
        for r in self.data.rows.iter_mut() {
            chemchron_core::reference::recompute_ref_fields_for_row(r, &refs, &dp);
        }
        self.data.refresh(&self.settings);
    }

    /// What Undo would take back.
    pub fn undo_label(&self) -> Option<String> {
        self.history.order.iter().rev().find_map(|k| match k {
            Kind::Data => self.data.can_undo().map(|w| w.to_string()),
            Kind::Laser => self.laser.can_undo().then(|| "laser session edit".to_string()),
            Kind::Settings => (!self.history.settings_undo.is_empty()).then(|| "settings change".to_string()),
        })
    }

    pub fn redo_label(&self) -> Option<String> {
        self.history.future.iter().rev().find_map(|k| match k {
            Kind::Data => self.data.can_redo().map(|w| w.to_string()),
            Kind::Laser => self.laser.can_redo().then(|| "laser session edit".to_string()),
            Kind::Settings => (!self.history.settings_redo.is_empty()).then(|| "settings change".to_string()),
        })
    }

    /// Takes back the latest change, in whichever part it was made.
    pub fn undo(&mut self) {
        self.track_history();
        while let Some(k) = self.history.order.pop() {
            let done = match k {
                Kind::Data if self.data.can_undo().is_some() => {
                    let what = self.data.can_undo().unwrap_or_default().to_string();
                    self.data.undo();
                    self.data.refresh(&self.settings);
                    self.say(format!("Undone: {what}"), false);
                    true
                }
                Kind::Laser if self.laser.can_undo() => {
                    self.laser.undo();
                    self.say("Undone: laser session edit", false);
                    true
                }
                Kind::Settings if !self.history.settings_undo.is_empty() => {
                    let prev = self.history.settings_undo.pop().unwrap();
                    self.history.settings_redo.push(std::mem::replace(&mut self.settings, prev));
                    self.settings_changed_by_undo();
                    self.say("Undone: settings change", false);
                    true
                }
                _ => false,
            };
            if done {
                self.history.future.push(k);
                return;
            }
        }
    }

    pub fn redo(&mut self) {
        while let Some(k) = self.history.future.pop() {
            let done = match k {
                Kind::Data if self.data.can_redo().is_some() => {
                    self.data.redo();
                    self.data.refresh(&self.settings);
                    true
                }
                Kind::Laser if self.laser.can_redo() => {
                    self.laser.redo();
                    true
                }
                Kind::Settings if !self.history.settings_redo.is_empty() => {
                    let next = self.history.settings_redo.pop().unwrap();
                    self.history.settings_undo.push(std::mem::replace(&mut self.settings, next));
                    self.settings_changed_by_undo();
                    true
                }
                _ => false,
            };
            if done {
                self.history.order.push(k);
                return;
            }
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal_centered(|ui| {
            logo(ui, &t);
            ui.label(RichText::new("ChemChron").strong().size(16.0));
            ui.add_space(10.0);
            let parts = [(Part::Laser, "LA-ICP-MS"), (Part::Epma, "EPMA"), (Part::Combine, "Combined results")];
            ui.spacing_mut().item_spacing.x = 2.0;
            for (p, label) in parts {
                let on = self.part == p;
                let text = if on { RichText::new(label).color(t.on_accent).strong().size(14.0) } else { RichText::new(label).size(14.0) };
                let b = egui::Button::new(text).fill(if on { t.accent } else { t.raised }).min_size(egui::vec2(96.0, 30.0));
                let hint = match p {
                    Part::Laser => "Laser data evaluation: raw LA-ICP-MS sweeps to U–Th–Pb ratios and trace elements",
                    Part::Epma => "EPMA data evaluation: chemical Th–U–Pb ages and mineral formulae",
                    Part::Combine => "Bring laser and EPMA data together: Summary, concordia, REE, long-term logs, exports",
                    Part::Settings => "",
                };
                if ui.add(b).on_hover_text(hint).clicked() {
                    self.part = p;
                }
            }
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.separator();
            ui.label(RichText::new(self.project_name()).color(t.dim));
            if self.unsaved() {
                ui.label(RichText::new("• unsaved").color(t.dim).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("About").on_hover_text("About ChemChron and how to use it").clicked() {
                    self.help_open = true;
                }
                let on = self.part == Part::Settings;
                let label = if on { RichText::new("Settings").color(t.on_accent).strong() } else { RichText::new("Settings") };
                if ui
                    .add(egui::Button::new(label).fill(if on { t.accent } else { t.raised }))
                    .on_hover_text("Lab settings for all parts: working directory, reference materials, decay constants, colours")
                    .clicked()
                {
                    self.part = Part::Settings;
                }
                ui.separator();
                {
                    let redo = self.redo_label();
                    if ui.add_enabled(redo.is_some(), egui::Button::new("Redo")).on_hover_text(redo.map(|u| format!("Redo: {u} (Ctrl+Shift+Z)")).unwrap_or_default()).clicked() {
                        self.redo();
                    }
                    let undo = self.undo_label();
                    if ui.add_enabled(undo.is_some(), egui::Button::new("Undo")).on_hover_text(undo.map(|u| format!("Undo: {u} (Ctrl+Z, in every part)")).unwrap_or_default()).clicked() {
                        self.undo();
                    }
                    ui.separator();
                }
                ui.menu_button("Project", |ui| {
                    if ui.button("New project").clicked() {
                        ui.close();
                        self.new_project();
                    }
                    if ui.button("Open project…").on_hover_text("ChemChron projects and laser sessions").clicked() {
                        ui.close();
                        self.open_project_dialog();
                    }
                    let mut keep = self.settings.laser_in_project;
                    if ui
                        .checkbox(&mut keep, "Keep the laser session in the project")
                        .on_hover_text("Saving the project also saves the LA-ICP-MS session (selections, corrections, settings; raw files are referenced); opening the project opens it again for changes")
                        .changed()
                    {
                        self.settings.laser_in_project = keep;
                        self.save_settings();
                    }
                    ui.separator();
                    ui.menu_button("Open recent", |ui| {
                        let recent = self.settings.recent_projects.clone();
                        if recent.is_empty() {
                            ui.label("(no recent projects)");
                        }
                        for p in recent {
                            if ui.button(&p).clicked() {
                                ui.close();
                                if Path::new(&p).is_file() {
                                    self.open_project(Path::new(&p));
                                } else {
                                    self.settings.recent_projects.retain(|x| *x != p);
                                    self.save_settings();
                                    self.say(format!("{p} is no longer there; removed from the list."), true);
                                }
                            }
                        }
                        ui.separator();
                        if ui.button("Clear recent projects").clicked() {
                            self.settings.recent_projects.clear();
                            self.save_settings();
                            ui.close();
                        }
                    });
                    ui.separator();
                    if ui.button("Save project").clicked() {
                        ui.close();
                        self.save_project(false);
                    }
                    if ui.button("Save project as…").clicked() {
                        ui.close();
                        self.save_project(true);
                    }
                });
                if ui.button("Save").on_hover_text("Save the whole project: laser session, EPMA data and links (Ctrl+S)").clicked() {
                    self.save_project(false);
                }
            });
        });
    }

    pub fn status_line(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal_centered(|ui| {
            if let Some((text, problem)) = self.notice.clone() {
                ui.label(RichText::new(text).color(if problem { t.alarm } else { t.dim }).size(12.0));
            } else {
                let n = self.data.rows.len();
                let refs = self.data.rows.iter().filter(|r| truthy(r, "Is_Reference")).count();
                let upb = self.data.rows.iter().filter(|r| chemchron_core::upb::has_upb(r)).count();
                ui.label(
                    RichText::new(format!(
                        "{n} analyses · {refs} reference · {upb} with U–Pb · {} trace elements · settings in {}",
                        self.data.trace.len(),
                        self.settings.dir.display()
                    ))
                    .color(t.dim)
                    .size(12.0),
                );
            }
        });
    }

    fn close_dialog(&mut self, ctx: &Context) {
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && self.unsaved() && !self.allow_close && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            let t = self.theme();
            egui::Modal::new(egui::Id::unique("cc_confirm_close")).show(ctx, |ui| {
                ui.set_width(360.0);
                let laser_apart = self.laser_unsaved() && !self.settings.laser_in_project;
                let what = match (self.data.unsaved, laser_apart) {
                    (true, true) => "Save the project and the laser session before closing?",
                    (false, true) => "Save the laser session before closing?",
                    _ => "Save this project before closing?",
                };
                ui.label(RichText::new(what).strong());
                if laser_apart {
                    ui.label(RichText::new("The laser session isn't kept in the project (Project menu), so it is saved as its own file.").color(t.dim));
                }
                ui.label(RichText::new("Your raw data files are safe either way. Changes since the last save would be lost.").color(t.dim));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if self.data.unsaved || self.settings.laser_in_project {
                            self.save_project(false);
                        }
                        if self.laser_unsaved() && !self.settings.laser_in_project {
                            self.laser.save(None);
                        }
                        if !self.unsaved() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        self.confirm_close = false;
                    }
                    if ui.button("Don't save").clicked() {
                        crate::autosave::clear(self);
                        self.allow_close = true;
                        self.confirm_close = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
        }
    }

    fn drive_screenshot(&mut self, ctx: &Context) {
        let frames = self.frames;
        let Some((path, requested, done)) = &mut self.screenshot else { return };
        ctx.request_repaint();
        if frames >= 120 && !*requested {
            *requested = true;
            let p = path.clone();
            let d = done.clone();
            let cb = egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
                *d.lock().unwrap() = Some(save_png(&p, &image));
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        let fin = done.lock().unwrap().take();
        if let Some(r) = fin {
            if let Err(e) = r {
                eprintln!("{e}");
                std::process::exit(1);
            }
            self.allow_close = true;
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn logo(ui: &mut Ui, t: &Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    p.circle_stroke(c, 10.0, Stroke::new(2.0, t.accent));
    p.circle_filled(c + egui::vec2(-3.5, 2.0), 2.6, t.accent);
    p.circle_filled(c + egui::vec2(4.0, -3.0), 2.6, Color32::from_rgb(227, 169, 72));
    p.line_segment([c + egui::vec2(-7.0, 6.0), c + egui::vec2(7.0, -6.0)], Stroke::new(1.5, t.accent));
}

impl eframe::App for ChemApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.ui_root(ui);
    }
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        let c = self.theme().bg;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }
}

pub fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let [w, h] = image.size;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for p in &image.pixels {
        bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
    }
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| format!("{}: {e}", path.display()))
}

/// Navigation list in the left panel with status dots.
pub fn nav<P: PartialEq + Copy>(ui: &mut Ui, t: &Theme, current: &mut P, pages: &[(P, &str, Option<Color32>, bool)]) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut number = 0;
    for (p, title, dot, numbered) in pages {
        if !numbered && number > 0 {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            number = -1000;
        }
        if *numbered {
            number += 1;
        }
        let on = *current == *p;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 31.0), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *title));
        if on {
            ui.painter().rect_filled(rect, 5, t.raised);
            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(3.0, 19.0)), 2, t.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 5, t.raised.linear_multiply(0.5));
        }
        let label = if *numbered && number > 0 { format!("{number}  {title}") } else { title.to_string() };
        ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.5), if on { t.text } else { t.dim });
        ui.painter().circle_filled(rect.right_center() - egui::vec2(12.0, 0.0), 3.5, dot.unwrap_or(t.border));
        if resp.clicked() {
            *current = *p;
        }
    }
}

/// The left navigation, bottom status and central page frame shared by EPMA and Combine.
pub fn frame_part(app: &mut ChemApp, ui: &mut Ui, id: &str, nav_ui: impl FnOnce(&mut ChemApp, &mut Ui), page: impl FnOnce(&mut ChemApp, &mut Ui)) {
    let t = app.theme();
    egui::Panel::bottom(format!("{id}_status"))
        .exact_size(26.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(12, 4)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| app.status_line(ui));
    egui::Panel::left(format!("{id}_nav"))
        .exact_size(196.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(8, 10)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| nav_ui(app, ui));
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg).inner_margin(Margin::same(10))).show(ui, |ui| page(app, ui));
}
